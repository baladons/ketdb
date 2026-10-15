use std::fs;

use compio::runtime::Runtime;
use format::{
    context::Context,
    footer::BlockEncoding,
    table_reader::TableReader,
    table_writer::{ColumnConfig, TableWriter},
    types::{BlockType, KEY_COLUMN_ID},
};
use uuid::Uuid;

fn bytes(value: u32) -> [u8; 4] {
    value.to_le_bytes()
}

#[test]
fn table_writer_round_trips_data_and_indexing() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    std::env::set_current_dir(manifest_dir).unwrap();
    fs::create_dir_all("data").unwrap();

    let table_id = Uuid::from_u128(0x1234_5678_9abc_def0_1234_5678_9abc_def0);
    let file_path = format!("data/{}.ket", table_id);
    let _ = fs::remove_file(&file_path);
    fs::File::create(&file_path).unwrap();

    Context::new(8).into_current();

    Runtime::new().unwrap().block_on(async {
        let mut writer = TableWriter::new(
            table_id,
            vec![
                ColumnConfig {
                    column_id: KEY_COLUMN_ID,
                    block_size: 12,
                    block_encoding: BlockEncoding {
                        key_id: 0,
                        compression: 0,
                    },
                },
                ColumnConfig {
                    column_id: 1,
                    block_size: 12,
                    block_encoding: BlockEncoding {
                        key_id: 0,
                        compression: 0,
                    },
                },
            ],
        );

        let rows = [
            (1_u32, 10_u32),
            (2_u32, 20_u32),
            (3_u32, 30_u32),
            (4_u32, 40_u32),
            (5_u32, 50_u32),
        ];

        for (key, value) in rows {
            writer.write(BlockType::Data, KEY_COLUMN_ID, &bytes(key)).await;
            writer.write(BlockType::Data, 1, &bytes(value)).await;
        }

        writer.finish().await;

        let reader = TableReader::new(table_id);

        assert_eq!(reader.len(BlockType::Data, KEY_COLUMN_ID).await, 2);
        assert_eq!(reader.len(BlockType::Min, KEY_COLUMN_ID).await, 1);
        assert_eq!(reader.len(BlockType::Max, KEY_COLUMN_ID).await, 1);
        assert_eq!(reader.len(BlockType::Bloom, KEY_COLUMN_ID).await, 1);

        assert_eq!(reader.len(BlockType::Data, 1).await, 2);
        assert_eq!(reader.len(BlockType::Min, 1).await, 1);
        assert_eq!(reader.len(BlockType::Max, 1).await, 1);
        assert_eq!(reader.len(BlockType::Bloom, 1).await, 1);

        assert_eq!(reader.row_index_by_key(&bytes(1)).await, 0);
        assert_eq!(reader.row_index_by_key(&bytes(4)).await, 3);
        assert_eq!(reader.row_index_by_key(&bytes(5)).await, 4);

        let first_key_block = reader.read(BlockType::Data, KEY_COLUMN_ID, 0).await;
        assert_eq!(first_key_block.count(), 3);
        assert_eq!(first_key_block.get(0), &bytes(1));
        assert_eq!(first_key_block.get(2), &bytes(3));

        let second_key_block = reader.read(BlockType::Data, KEY_COLUMN_ID, 1).await;
        assert_eq!(second_key_block.count(), 2);
        assert_eq!(second_key_block.get(0), &bytes(4));
        assert_eq!(second_key_block.get(1), &bytes(5));

        let first_value_block = reader.read(BlockType::Data, 1, 0).await;
        assert_eq!(first_value_block.count(), 3);
        assert_eq!(first_value_block.get(0), &bytes(10));
        assert_eq!(first_value_block.get(2), &bytes(30));

        let second_value_block = reader.read(BlockType::Data, 1, 1).await;
        assert_eq!(second_value_block.count(), 2);
        assert_eq!(second_value_block.get(0), &bytes(40));
        assert_eq!(second_value_block.get(1), &bytes(50));
    });

    let _ = fs::remove_file(&file_path);
}
