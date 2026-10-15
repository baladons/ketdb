use crate::{
    batch::Batch,
    block_reader::BlockReader,
    table_reader::TableReader,
    types::{BlockIndex, ColumnId, RowIndex},
};

pub struct BlockCursor {
    table: TableReader,
    block: BlockReader,
    column_id: ColumnId,
    block_index: BlockIndex,
    internal_index: RowIndex,
}

impl BlockCursor {
    pub async fn new(table: TableReader, column_id: ColumnId, row_index: RowIndex) -> Self {
        let block_index = table
            .data_block_index_by_row_index(column_id, row_index)
            .await;

        let block = table
            .read(crate::types::BlockType::Data, column_id, block_index)
            .await
            .into();

        let start_row_index = table
            .start_row_index(crate::types::BlockType::Data, column_id, block_index)
            .await;

        let internal_index = row_index - start_row_index;

        Self {
            table,
            block,
            column_id,
            block_index,
            internal_index,
        }
    }

    async fn next_block(&mut self) {
        self.block_index += 1;
        self.block = self
            .table
            .read(
                crate::types::BlockType::Data,
                self.column_id,
                self.block_index,
            )
            .await
            .into();
        self.internal_index = 0;
    }

    pub async fn next(&mut self, batch_size: u32) -> Batch<'_> {
        if self.internal_index >= self.block.count() {
            self.next_block().await;
        }

        let batch = self.block.batch(self.internal_index, batch_size);
        self.internal_index += batch.len() as u32;

        batch
    }
}
