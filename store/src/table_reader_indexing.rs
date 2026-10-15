use crate::{
    binary_search::binary_search,
    block_reader::BlockReader,
    table_reader::TableReader,
    types::{BlockIndex, BlockType, ColumnId, KEY_COLUMN_ID, RowIndex},
};

impl TableReader {
    pub async fn min_block_index_by_key(&self, key: &[u8]) -> BlockIndex {
        let block_count = self.len(BlockType::Min, KEY_COLUMN_ID).await;
        if block_count == 0 {
            return 0;
        }

        let block_index = binary_search(block_count, async |i| {
            let block = self.read(BlockType::Min, KEY_COLUMN_ID, i).await;
            let block_reader = BlockReader::from(block);
            block_reader.get(0).cmp(key)
        })
        .await;

        if block_index == 0 {
            0
        } else if block_index >= block_count {
            block_count - 1
        } else {
            let block = self.read(BlockType::Min, KEY_COLUMN_ID, block_index).await;
            let block_reader = BlockReader::from(block);

            if block_reader.get(0) > key {
                block_index - 1
            } else {
                block_index
            }
        }
    }

    pub async fn row_index_by_key(&self, key: &[u8]) -> RowIndex {
        let data_block_count = self.len(BlockType::Data, KEY_COLUMN_ID).await;
        if data_block_count == 0 {
            return 0;
        }

        let min_block_index = self.min_block_index_by_key(key).await;

        let min_block: BlockReader = self
            .read(BlockType::Min, KEY_COLUMN_ID, min_block_index)
            .await
            .into();

        let data_block_index = min_block.internal_index_by_key(key).await.min(data_block_count - 1);

        let data_block_start = self
            .start_row_index(BlockType::Data, KEY_COLUMN_ID, data_block_index)
            .await;

        let data_block: BlockReader = self
            .read(BlockType::Data, KEY_COLUMN_ID, data_block_index)
            .await
            .into();

        let index_in_block = data_block.internal_index_by_key(key).await;

        data_block_start + index_in_block
    }

    pub async fn data_block_index_by_row_index(
        &self,
        column_id: ColumnId,
        row_index: RowIndex,
    ) -> BlockIndex {
        let row_index = row_index.to_le_bytes();

        binary_search(self.len(BlockType::Data, column_id).await, async |i| {
            self.start_row_index(BlockType::Data, column_id, i)
                .await
                .cmp(&u32::from_le_bytes(row_index))
        })
        .await
    }
}
