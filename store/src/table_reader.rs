use uuid::Uuid;

use crate::{
    block_reader::BlockReader,
    context::Context,
    types::{BlockIndex, BlockType, ColumnId, RowIndex},
};

#[derive(Clone)]
pub struct TableReader {
    id: Uuid,
}

impl TableReader {
    pub fn new(id: Uuid) -> Self {
        Self { id }
    }

    pub async fn start_row_index(
        &self,
        block_type: BlockType,
        column_id: ColumnId,
        block_index: BlockIndex,
    ) -> RowIndex {
        let footer = Context::current().footer_cache.get(self.id).await;
        let handle = footer.block_handle(column_id, block_type, block_index);
        handle.start_row_index
    }

    pub async fn read(
        &self,
        block_type: BlockType,
        column_id: ColumnId,
        block_index: BlockIndex,
    ) -> BlockReader {
        Context::current()
            .block_cache
            .get(self.id, block_type, column_id, block_index)
            .await
    }

    pub async fn len(&self, block_type: BlockType, column_id: ColumnId) -> BlockIndex {
        let footer = Context::current().footer_cache.get(self.id).await;
        footer.block_handles(column_id, block_type).len() as BlockIndex
    }
}
