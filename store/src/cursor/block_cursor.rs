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

    /// Advance up to `count` rows within this segment column. Returns how many could not
    /// be advanced (e.g. asked for 20, advanced 5 → returns 15).
    pub async fn advance(
        &mut self,
        segment: &Segment,
        storage: &mut Storage,
        count: u32,
    ) -> Result<u32, Error> {
        let mut remaining = count;

        loop {
            let block_end = self.row_offset + self.block.len() as u32;

            if remaining > 0 && self.row_index < block_end {
                let rows_left = block_end - self.row_index;
                let step = remaining.min(rows_left);
                self.row_index += step;
                remaining -= step;
                continue;
            }

            if self.row_index < block_end {
                return Ok(remaining);
            }

            let column = segment
                .column(self.column_id)
                .ok_or(Error::DeserializationFailure)?;
            let handles = column.data_blocks.as_slice();

            let mut block_index = self.block_index;
            let mut loaded = false;
            while block_index < handles.len() {
                let start = handles[block_index].row_offset.to_native();
                if start > self.row_index {
                    break;
                }
                let end = start + handles[block_index].row_len.to_native();
                if self.row_index < end {
                    let (block, handle) = segment
                        .block(storage, BlockKind::Data, self.column_id, block_index)
                        .await?;

                    self.row_offset = handle.row_offset.to_native();
                    self.block_index = block_index;
                    self.block = block;
                    loaded = true;
                    break;
                }
                block_index += 1;
            }

            if !loaded {
                if let Some(last) = handles.last() {
                    let segment_end = last.row_offset.to_native() + last.row_len.to_native();
                    let last_row = segment_end - 1;
                    if self.row_index > last_row {
                        let overshoot = self.row_index - last_row;
                        self.row_index = last_row;
                        if remaining == 0 {
                            return Ok(overshoot);
                        }
                        return Ok(remaining);
                    }
                }
                return Ok(remaining);
            }

            if remaining == 0 {
                return Ok(0);
            }
        }
    }
}
