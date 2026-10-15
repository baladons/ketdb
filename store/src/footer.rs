use rkyv::{Archive, Deserialize, Serialize};

use crate::types::{BlockIndex, BlockType, ColumnId};

#[derive(Clone, Archive, Deserialize, Serialize)]
pub struct BlockEncoding {
    pub key_id: u32,     // 0 is no key and hence no AES encryption
    pub compression: u8, // 0 is no ZSTD compression
}

#[derive(Clone, Archive, Deserialize, Serialize)]
pub struct BlockHandle {
    pub start_row_index: u32,
    pub start_offset: u64,
    /// Encoded block length in bytes (not including page padding written to disk).
    pub end_offset: u64,
    pub checksum: u32,
    pub encoding: BlockEncoding,
    pub uncompressed_size: u32,
}

#[derive(Archive, Deserialize, Serialize, Default)]
pub struct ColumnHandle {
    pub column_id: ColumnId,
    pub data_blocks: Vec<BlockHandle>,
    pub min_blocks: Vec<BlockHandle>,
    pub max_blocks: Vec<BlockHandle>,
    pub bloom_blocks: Vec<BlockHandle>,
}

#[derive(Archive, Deserialize, Serialize, Default)]
pub struct Footer {
    pub columns: Vec<ColumnHandle>, // not necessary sorted by column id
}

impl Footer {
    pub fn contains_column(&self, column_id: ColumnId) -> bool {
        self.columns
            .iter()
            .any(|column| column.column_id == column_id)
    }

    pub fn column(&self, column_id: ColumnId) -> &ColumnHandle {
        self.columns
            .iter()
            .find(|column| column.column_id == column_id)
            .expect("missing column")
    }

    pub fn column_mut(&mut self, column_id: ColumnId) -> &mut ColumnHandle {
        if let Some(index) = self
            .columns
            .iter()
            .position(|column| column.column_id == column_id)
        {
            return &mut self.columns[index];
        }

        self.columns.push(ColumnHandle {
            column_id,
            ..Default::default()
        });

        self.columns.last_mut().expect("missing column")
    }

    pub fn block_handles(&self, column_id: ColumnId, block_type: BlockType) -> &[BlockHandle] {
        let column = self.column(column_id);

        match block_type {
            BlockType::Data => &column.data_blocks,
            BlockType::Min => &column.min_blocks,
            BlockType::Max => &column.max_blocks,
            BlockType::Bloom => &column.bloom_blocks,
        }
    }

    pub fn block_handles_mut(
        &mut self,
        column_id: ColumnId,
        block_type: BlockType,
    ) -> &mut Vec<BlockHandle> {
        let column = self.column_mut(column_id);

        match block_type {
            BlockType::Data => &mut column.data_blocks,
            BlockType::Min => &mut column.min_blocks,
            BlockType::Max => &mut column.max_blocks,
            BlockType::Bloom => &mut column.bloom_blocks,
        }
    }

    pub fn block_handle(
        &self,
        column_id: ColumnId,
        block_type: BlockType,
        block_index: BlockIndex,
    ) -> &BlockHandle {
        &self.block_handles(column_id, block_type)[block_index as usize]
    }
}
