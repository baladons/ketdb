use fastbloom::BloomFilter;
use uuid::Uuid;

use crate::{
    block_reader::BlockReader,
    context::Context,
    types::{BlockIndex, BlockType, ColumnId, RowIndex, BLOOM_SEED},
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

    pub async fn has_value(&self, column_id: ColumnId, value: &[u8]) -> bool {
        let footer = Context::current().footer_cache.get(self.id).await;
        if !footer.contains_column(column_id)
            || footer.block_handles(column_id, BlockType::Bloom).is_empty()
        {
            return false;
        }

        let bytes = self.read(BlockType::Bloom, column_id, 0).await;
        let bytes = bytes.as_bytes();
        if bytes.len() < 4 {
            return false;
        }

        let num_hashes = u32::from_le_bytes(bytes[..4].try_into().unwrap());
        let words = bytes[4..]
            .chunks_exact(8)
            .map(|chunk| u64::from_le_bytes(chunk.try_into().unwrap()))
            .collect::<Vec<_>>();
        if words.is_empty() {
            return false;
        }

        BloomFilter::from_vec(words)
            .seed(&BLOOM_SEED)
            .hashes(num_hashes)
            .contains(value)
    }

    pub async fn len(&self, block_type: BlockType, column_id: ColumnId) -> BlockIndex {
        let footer = Context::current().footer_cache.get(self.id).await;
        footer.block_handles(column_id, block_type).len() as BlockIndex
    }
}
