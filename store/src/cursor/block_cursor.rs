use crate::{
    batch::Batch,
    block::Block,
    error::Error,
    segment::{BlockKind, Segment},
    context::Context,
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
        context: &mut Context,
    ) -> Result<Self, Error> {
        let block_index = segment.block_index_from_row_index(column_id, row_index)?;

        let (block, handle) = segment
            .block(context, BlockKind::Data, column_id, block_index)
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
        self.block.batch(self.index_in_block())
    }

    pub fn index_in_block(&self) -> usize {
        (self.row_index - self.row_offset) as usize
    }

    pub async fn advance(
        &mut self,
        segment: &Segment,
        context: &mut Context,
        count: u32,
    ) -> Result<u32, Error> {
        let mut remaining = count;

        loop {
            let rows_left = self.block.row_count().saturating_sub(self.index_in_block()) as u32;

            if remaining < rows_left {
                self.row_index += remaining;
                return Ok(0);
            }

            remaining -= rows_left;
            self.row_index += rows_left;
            self.block_index += 1;

            let column = segment
                .column(self.column_id)
                .ok_or(Error::DeserializationFailure)?;

            if self.block_index >= column.data_blocks.len() {
                return Ok(remaining);
            }

            let (block, handle) = segment
                .block(context, BlockKind::Data, self.column_id, self.block_index)
                .await?;

            self.row_offset = handle.row_offset.to_native();
            self.row_index = self.row_offset;
            self.block = block;
        }
    }
}
