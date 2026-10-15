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

        match binary_search(block_count, async |i| {
            let block = self.read(BlockType::Min, KEY_COLUMN_ID, i).await;
            let block_reader = BlockReader::from(block);
            block_reader.get(0).cmp(key)
        })
        .await
        {
            Ok(index) => index,
            Err(0) => 0,
            Err(index) if index >= block_count => block_count - 1,
            Err(index) => index - 1,
        }
    }

    pub async fn row_index_by_key(&self, key: &[u8]) -> Result<RowIndex, RowIndex> {
        let data_block_count = self.len(BlockType::Data, KEY_COLUMN_ID).await;
        if data_block_count == 0 {
            return Err(0);
        }

        let min_block_index = self.min_block_index_by_key(key).await;

        let min_block: BlockReader = self
            .read(BlockType::Min, KEY_COLUMN_ID, min_block_index)
            .await
            .into();

        let min_block_start_data_block_index = self
            .start_row_index(BlockType::Min, KEY_COLUMN_ID, min_block_index)
            .await;

        let index_in_min_block = match min_block.internal_index_by_key(key).await {
            Ok(index) => index,
            Err(index) => index.saturating_sub(1),
        };
        let data_block_index = (min_block_start_data_block_index + index_in_min_block)
            .min(data_block_count - 1);

        let data_block_start = self
            .start_row_index(BlockType::Data, KEY_COLUMN_ID, data_block_index)
            .await;

        let data_block: BlockReader = self
            .read(BlockType::Data, KEY_COLUMN_ID, data_block_index)
            .await
            .into();

        match data_block.internal_index_by_key(key).await {
            Ok(index) => Ok(data_block_start + index),
            Err(index) => Err(data_block_start + index),
        }
    }

    pub async fn data_block_index_by_row_index(
        &self,
        column_id: ColumnId,
        row_index: RowIndex,
    ) -> BlockIndex {
        let block_count = self.len(BlockType::Data, column_id).await;
        if block_count == 0 {
            return 0;
        }

        match binary_search(block_count, async |i| {
            self.start_row_index(BlockType::Data, column_id, i)
                .await
                .cmp(&row_index)
        })
        .await
        {
            Ok(index) => index,
            Err(index) => index.saturating_sub(1),
        }
    }
}
