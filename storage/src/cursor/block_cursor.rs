use crate::{
    batch::Batch,
    block::Block,
    error::Error,
    segment::{BlockKind, Segment},
    storage::Storage,
};

pub struct BlockCursor {
    column_id: u32,
    row_index: u32,
    row_offset: u32,
    block_index: usize,
    block: Block,
}

impl BlockCursor {
    pub async fn at_row_index(
        segment: &Segment,
        column_id: u32,
        row_index: u32,
        storage: &mut Storage,
    ) -> Result<Self, Error> {
        let block_index = segment.block_index_from_row_index(column_id, row_index)?;

        let (block, handle) = segment
            .block(storage, BlockKind::Data, column_id, block_index)
            .await?;

        Ok(Self {
            column_id,
            row_index,
            row_offset: handle.row_offset.to_native(),
            block_index,
            block,
        })
    }

    pub fn row_index(&self) -> u32 {
        self.row_index
    }

    pub fn column_id(&self) -> u32 {
        self.column_id
    }

    pub fn block(&self) -> &Block {
        &self.block
    }

    pub fn batch(&self) -> Batch<'_> {
        self.block
            .batch((self.row_index - self.row_offset) as usize)
    }

    pub async fn advance(
        &mut self,
        segment: &Segment,
        storage: &mut Storage,
        count: u32,
    ) -> Result<bool, Error> {
        self.row_index += count;

        if self.row_index < self.row_offset + self.block.len() as u32 {
            return Ok(true);
        }

        let block_index = match segment.block_index_from_row_index(self.column_id, self.row_index) {
            Ok(index) => index,
            Err(_) => {
                self.row_index -= count;
                return Ok(false);
            }
        };

        let (block, handle) = segment
            .block(storage, BlockKind::Data, self.column_id, block_index)
            .await?;

        self.row_offset = handle.row_offset.to_native();
        self.block_index = block_index;
        self.block = block;

        Ok(true)
    }
}
