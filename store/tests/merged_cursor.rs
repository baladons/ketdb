use std::fs;

use compio::runtime::Runtime;
use format::{
    context::Context,
    footer::BlockEncoding,
    merged_table_cursor::MergedTableCursor,
    table_cursor::TableCursor,
    table_reader::TableReader,
    table_writer::{ColumnConfig, TableWriter},
    types::{BlockType, KEY_COLUMN_ID},
};
use uuid::Uuid;

const ROW_COUNT: u32 = 20;

fn u32_bytes(value: u32) -> [u8; 4] {
    value.to_le_bytes()
}

async fn write_table(table_id: Uuid, block_size: usize, value_scale: u32) {
    let file_path = format!("data/{}.ket", table_id);
    let _ = fs::remove_file(&file_path);
    fs::File::create(&file_path).unwrap();

    let encoding = BlockEncoding {
        key_id: 0,
        compression: 0,
    };
    let mut writer = TableWriter::new(
        table_id,
        vec![
            ColumnConfig {
                column_id: KEY_COLUMN_ID,
                block_size,
                block_encoding: encoding.clone(),
                bloom_expected_items: ROW_COUNT as usize,
            },
            ColumnConfig {
                column_id: 1,
                block_size,
                block_encoding: encoding,
                bloom_expected_items: ROW_COUNT as usize,
            },
        ],
    );

    for key in 1..=ROW_COUNT {
        writer
            .write(BlockType::Data, KEY_COLUMN_ID, &u32_bytes(key))
            .await;
        writer
            .write(BlockType::Data, 1, &u32_bytes(key * value_scale))
            .await;
    }
    writer.finish().await;
}

#[test]
fn merged_cursor_interleaves_three_tables() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    std::env::set_current_dir(manifest_dir).unwrap();
    fs::create_dir_all("data").unwrap();
    Context::new(8).into_current();

    let tables = [
        (Uuid::from_u128(1), 12, 10),
        (Uuid::from_u128(2), 8, 100),
        (Uuid::from_u128(3), 20, 1000),
    ];

    Runtime::new().unwrap().block_on(async {
        for (id, block_size, scale) in tables {
            write_table(id, block_size, scale).await;
        }

        let readers = tables.map(|(id, _, _)| TableReader::new(id));
        let columns = [KEY_COLUMN_ID, 1];
        let inputs = [
            (readers[0].clone(), columns.as_slice()),
            (readers[1].clone(), columns.as_slice()),
            (readers[2].clone(), columns.as_slice()),
        ];

        let table_cursor =
            TableCursor::seek_to_row_index(readers[0].clone(), 0, &columns).await;
        assert!(table_cursor.has_value(1, &u32_bytes(10)).await);
        assert!(!table_cursor.has_value(1, &u32_bytes(15)).await);

        let mut cursor = MergedTableCursor::seek_to_row_index(&inputs, 0).await;
        assert!(cursor.has_value(1, &u32_bytes(10)).await);
        assert!(cursor.has_value(1, &u32_bytes(100)).await);
        assert!(cursor.has_value(1, &u32_bytes(1000)).await);
        assert!(!cursor.has_value(1, &u32_bytes(15)).await);
        let mut seen = 0u32;

        loop {
            let batch = cursor.next(5).await;
            if batch.len() == 0 {
                break;
            }
            assert_eq!(batch.cells_per_row(), 6);

            for row in 0..batch.len() {
                let key = seen + 1;
                assert_eq!(batch.get(row, 0), u32_bytes(key));
                assert_eq!(batch.get(row, 1), u32_bytes(key * 10));
                assert_eq!(batch.get(row, 2), u32_bytes(key));
                assert_eq!(batch.get(row, 3), u32_bytes(key * 100));
                assert_eq!(batch.get(row, 4), u32_bytes(key));
                assert_eq!(batch.get(row, 5), u32_bytes(key * 1000));
                seen += 1;
            }
        }

        assert_eq!(seen, ROW_COUNT);
    });

    for (id, _, _) in tables {
        let _ = fs::remove_file(format!("data/{}.ket", id));
    }
}
