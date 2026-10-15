mod common;

use compio::runtime::Runtime;
use format::{
    delete_mask::DeleteMask,
    footer::BlockEncoding,
    metadata,
    mem_table::{MemTable, MEMTABLE_MAX_ENTRIES},
    merged_table_cursor::MergedTableCursor,
    table_reader::TableReader,
    table_writer::ColumnConfig,
    types::{BlockType, KEY_COLUMN_ID},
};

fn configs() -> Vec<ColumnConfig> {
    let encoding = BlockEncoding {
        key_id: 0,
        compression: 0,
    };
    vec![
        ColumnConfig {
            column_id: KEY_COLUMN_ID,
            block_size: 32,
            block_encoding: encoding.clone(),
            bloom_expected_items: 1,
        },
        ColumnConfig {
            column_id: 1,
            block_size: 32,
            block_encoding: encoding,
            bloom_expected_items: 1,
        },
    ]
}

#[test]
fn memtable_flushes_into_linked_tables_and_replays_wal() {
    let _lock = common::DataLock::acquire();
    assert_eq!(MEMTABLE_MAX_ENTRIES, 100_000);

    Runtime::new().unwrap().block_on(async {
        let first_table = {
            let mut memtable = MemTable::open_with_limit(configs(), 3).await;
            memtable.insert(&[b"a", b"1"]).await;
            memtable.insert(&[b"c", b"3"]).await;
            memtable.insert(&[b"b", b"2"]).await;

            assert!(memtable.get(b"a").is_none());
            assert_eq!(memtable.len(), 0);

            let tables = memtable.flushed_tables().await;
            assert_eq!(tables.len(), 1);

            let reader = TableReader::new(tables[0]);
            assert_eq!(reader.row_index_by_key(b"a").await, Ok(0));
            assert_eq!(reader.row_index_by_key(b"b").await, Ok(1));
            assert_eq!(reader.row_index_by_key(b"c").await, Ok(2));

            let values = reader.read(BlockType::Data, 1, 0).await;
            assert_eq!(values.get(0), b"1");
            assert_eq!(values.get(1), b"2");
            assert_eq!(values.get(2), b"3");
            tables[0]
        };

        {
            let mut memtable = MemTable::open_with_limit(configs(), 3).await;
            assert!(memtable.get(b"a").is_none());
            memtable.insert(&[b"d", b"4"]).await;
            assert_eq!(memtable.get(b"d").unwrap()[1].as_slice(), b"4");
        }

        let mut memtable = MemTable::open_with_limit(configs(), 3).await;
        assert_eq!(memtable.get(b"d").unwrap()[1].as_slice(), b"4");
        memtable.insert(&[b"f", b"6"]).await;
        memtable.insert(&[b"e", b"5"]).await;

        let tables = memtable.flushed_tables().await;
        assert_eq!(tables.len(), 1);
        assert_ne!(tables[0], first_table);
        assert!(memtable.get(b"d").is_none());

        let reader = TableReader::new(tables[0]);
        assert_eq!(reader.row_index_by_key(b"a").await, Ok(0));
        assert_eq!(reader.row_index_by_key(b"b").await, Ok(1));
        assert_eq!(reader.row_index_by_key(b"c").await, Ok(2));
        assert_eq!(reader.row_index_by_key(b"d").await, Ok(3));
        assert_eq!(reader.row_index_by_key(b"e").await, Ok(4));
        assert_eq!(reader.row_index_by_key(b"f").await, Ok(5));

        memtable.insert(&[b"a", b"9"]).await;
        memtable.insert(&[b"g", b"7"]).await;

        let mut cursor = MergedTableCursor::from_memtable(&memtable).await;
        assert!(cursor.has_value(1, b"9").await);
        assert!(cursor.has_value(1, b"3").await);

        let mut merged = Vec::new();
        loop {
            let batch = cursor.next(3).await;
            if batch.len() == 0 {
                break;
            }
            assert_eq!(batch.cells_per_row(), 2);
            for row in 0..batch.len() {
                merged.push((batch.get(row, 0).to_vec(), batch.get(row, 1).to_vec()));
            }
        }
        assert_eq!(
            merged,
            vec![
                (b"a".to_vec(), b"9".to_vec()),
                (b"b".to_vec(), b"2".to_vec()),
                (b"c".to_vec(), b"3".to_vec()),
                (b"d".to_vec(), b"4".to_vec()),
                (b"e".to_vec(), b"5".to_vec()),
                (b"f".to_vec(), b"6".to_vec()),
                (b"g".to_vec(), b"7".to_vec()),
            ]
        );

        let mut cursor = MergedTableCursor::seek_memtable(&memtable, b"e").await;
        let batch = cursor.next(10).await;
        assert_eq!(batch.len(), 3);
        assert_eq!(batch.get(0, 0), b"e");
        assert_eq!(batch.get(1, 0), b"f");
        assert_eq!(batch.get(2, 0), b"g");

        memtable.delete(b"b").await;
        assert_eq!(metadata::load().await.group(0).deleted_rows, 0);
        let tables = memtable.flushed_tables().await;
        assert_eq!(tables.len(), 1);
        let rows = {
            let reader = TableReader::new(tables[0]);
            format::delete_mask::table_rows(&reader).await
        };
        assert_eq!(rows, 6);
        assert_eq!(DeleteMask::load(tables[0], rows).await.deleted_count(rows), 0);

        let mut cursor = MergedTableCursor::from_memtable(&memtable).await;
        let mut merged = Vec::new();
        loop {
            let batch = cursor.next(10).await;
            if batch.len() == 0 {
                break;
            }
            for row in 0..batch.len() {
                merged.push((batch.get(row, 0).to_vec(), batch.get(row, 1).to_vec()));
            }
        }
        assert_eq!(
            merged,
            vec![
                (b"a".to_vec(), b"9".to_vec()),
                (b"c".to_vec(), b"3".to_vec()),
                (b"d".to_vec(), b"4".to_vec()),
                (b"e".to_vec(), b"5".to_vec()),
                (b"f".to_vec(), b"6".to_vec()),
                (b"g".to_vec(), b"7".to_vec()),
            ]
        );
    });
}

#[test]
fn flushed_tombstone_sets_the_bit_in_that_table_file() {
    let _lock = common::DataLock::acquire();

    Runtime::new().unwrap().block_on(async {
        let mut memtable = MemTable::open_with_limit(configs(), 100).await;
        memtable.insert(&[b"a", b"1"]).await;
        memtable.insert(&[b"b", b"2"]).await;
        memtable.insert(&[b"c", b"3"]).await;
        memtable.flush().await;

        memtable.delete(b"b").await;
        memtable.delete(b"missing").await;
        memtable.flush().await;

        let tables = memtable.flushed_tables().await;
        assert_eq!(tables.len(), 1);
        let rows = format::delete_mask::table_rows(&TableReader::new(tables[0])).await;
        assert_eq!(rows, 3);
        let mask = DeleteMask::load(tables[0], rows).await;
        assert!(!mask.is_set(0));
        assert!(mask.is_set(1));
        assert!(!mask.is_set(2));
        assert_eq!(mask.deleted_count(rows), 1);
        assert_eq!(metadata::load().await.group(0).deleted_rows, 1);
    });
}
