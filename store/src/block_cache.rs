use mini_moka::unsync::Cache;
use uuid::Uuid;

use crate::{
    block_reader::BlockReader,
    context::Context,
    encoding::BlockDecoder,
    types::{BlockIndex, BlockType, ColumnId},
    virtual_file::VirtualFile,
};

#[derive(Clone, Hash, PartialEq, Eq)]
struct BlockKey {
    table_id: Uuid,
    column_id: ColumnId,
    block_type: BlockType,
    block_index: BlockIndex,
}

pub struct BlockCache {
    inner: Cache<BlockKey, BlockReader>,
    decoder: BlockDecoder,
}

impl BlockCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Cache::new(capacity as u64).into(),
            decoder: BlockDecoder::new(),
        }
    }

    pub async fn get(
        &mut self,
        table_id: Uuid,
        block_type: BlockType,
        column_id: ColumnId,
        block_index: BlockIndex,
    ) -> BlockReader {
        let key = BlockKey {
            table_id,
            column_id,
            block_type,
            block_index,
        };

        if let Some(block) = self.inner.get(&key) {
            return block.clone();
        }

        let footer = Context::current().footer_cache.get(table_id).await;
        let handle = footer.block_handle(column_id, block_type, block_index);

        let file = VirtualFile::new(table_id);
        let encoded = file
            .read(
                vec![0; (handle.end_offset - handle.start_offset) as usize],
                handle.start_offset as usize,
            )
            .await;

        let decoded = self.decoder.decode(
            &encoded,
            &handle.encoding,
            handle.uncompressed_size as usize,
        );

        let block = BlockReader::from(decoded.to_vec());

        assert_eq!(handle.checksum, block.checksum());

        self.inner.insert(key, block.clone());
        block
    }
}
