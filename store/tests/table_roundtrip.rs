use std::fs;

use compio::runtime::Runtime;
use format::{
    batch::{Batch, BatchLayout},
    context::Context,
    footer::BlockEncoding,
    table_cursor::TableCursor,
    table_reader::TableReader,
    table_writer::{ColumnConfig, TableWriter},
    types::{BlockType, KEY_COLUMN_ID},
};
use uuid::Uuid;

const ROW_COUNT: u32 = 1_000_000;
const BATCH_SIZE: u32 = 4096;

fn u32_bytes(value: u32) -> [u8; 4] {
    value.to_le_bytes()
}

fn batch_u32(batch: &Batch<'_>, index: usize) -> u32 {
    let width = match batch.layout {
        BatchLayout::Fixed(width) => width,
        BatchLayout::Empty(_) | BatchLayout::Variable(_) => panic!("expected fixed-width batch"),
    };
    let offset = index * width;
    u32::from_le_bytes(batch.data[offset..offset + width].try_into().unwrap())
}

#[test]
fn table_writer_round_trips_one_million_rows_via_cursor() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    std::env::set_current_dir(manifest_dir).unwrap();
    fs::create_dir_all("data").unwrap();

    let table_id = Uuid::from_u128(0x1234_5678_9abc_def0_1234_5678_9abc_def0);
    let file_path = format!("data/{}.ket", table_id);
    let _ = fs::remove_file(&file_path);
    fs::File::create(&file_path).unwrap();

    Context::new(8).into_current();

    Runtime::new().unwrap().block_on(async {
        let encoding = BlockEncoding {
            key_id: 0,
            compression: 0,
        };

        let mut writer = TableWriter::new(
            table_id,
            vec![
                ColumnConfig {
                    column_id: KEY_COLUMN_ID,
                    block_size: 12,
                    block_encoding: encoding.clone(),
                    bloom_expected_items: ROW_COUNT as usize,
                },
                ColumnConfig {
                    column_id: 1,
                    block_size: 12,
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
                .write(BlockType::Data, 1, &u32_bytes(key * 10))
                .await;
        }
        writer.finish().await;

        let reader = TableReader::new(table_id);
        let mut cursor =
            TableCursor::seek_to_row_index(reader, 0, &[KEY_COLUMN_ID, 1]).await;

        let mut batches = cursor.next(BATCH_SIZE).await;
        let mut batch_index = 0usize;

        for expected_key in 1..=ROW_COUNT {
            if batch_index >= batches[0].len() {
                batches = cursor.next(BATCH_SIZE).await;
                batch_index = 0;
                assert_ne!(
                    batches[0].len(),
                    0,
                    "cursor ended before row {}",
                    expected_key - 1
                );
            }

            let key = batch_u32(&batches[0], batch_index);
            let value = batch_u32(&batches[1], batch_index);
            assert_eq!(key, expected_key);
            assert_eq!(value, expected_key * 10);
            batch_index += 1;
        }
    });

    let _ = fs::remove_file(&file_path);
}
