use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::{
    batch::Batch,
    context::Context,
    error::Error,
    file_id::FileId,
    segment::{BlockKind, Segment},
};

use super::segment_cursor::SegmentCursor;

pub struct MergeCursor {
    heap: BinaryHeap<Reverse<SegmentCursor>>,
}

impl MergeCursor {
    pub async fn seek_first(
        file_ids: Vec<FileId>,
        column_ids: Vec<u32>,
        context: &mut Context,
    ) -> Result<Self, Error> {
        if !column_ids.contains(&0) {
            return Err(Error::DeserializationFailure);
        }

        let mut heap = BinaryHeap::with_capacity(file_ids.len());
        for file_id in file_ids {
            let segment = Segment::open(context, file_id).await?;
            heap.push(Reverse(
                SegmentCursor::seek_first(segment, column_ids.clone(), context).await?,
            ));
        }

        Ok(Self { heap })
    }

    pub async fn seek_key(
        file_ids: Vec<FileId>,
        column_ids: Vec<u32>,
        key: &[u8],
        context: &mut Context,
    ) -> Result<Self, Error> {
        if !column_ids.contains(&0) {
            return Err(Error::DeserializationFailure);
        }

        let mut heap = BinaryHeap::with_capacity(file_ids.len());
        for file_id in file_ids {
            let segment = Segment::open(context, file_id).await?;
            if !has_key_at_or_after(&segment, context, key).await? {
                continue;
            }
            heap.push(Reverse(
                SegmentCursor::seek_key(segment, column_ids.clone(), key, context).await?,
            ));
        }

        Ok(Self { heap })
    }

    /// Advance `count` merged rows. Returns how many could not be advanced.
    pub async fn advance(&mut self, context: &mut Context, count: u32) -> Result<u32, Error> {
        let mut remaining = count;
        while remaining > 0 {
            let Some(Reverse(mut cursor)) = self.heap.pop() else {
                return Ok(remaining);
            };

            cursor.advance(context, 1).await?;
            if !cursor.done() {
                self.heap.push(Reverse(cursor));
                remaining -= 1;
                continue;
            }

            if self.heap.is_empty() {
                return Ok(remaining);
            }
            remaining -= 1;
        }
        Ok(0)
    }

    pub fn done(&self) -> bool {
        self.heap.is_empty()
    }

    pub fn batch(&self) -> Vec<Batch<'_>> {
        let Some(Reverse(cursor)) = self.heap.peek() else {
            return Vec::new();
        };
        cursor
            .batch()
            .into_iter()
            .map(|batch| batch.truncate(1))
            .collect()
    }
}

async fn has_key_at_or_after(
    segment: &Segment,
    context: &mut Context,
    key: &[u8],
) -> Result<bool, Error> {
    let Some(column) = segment.column(0) else {
        return Ok(false);
    };
    let Some(last_index) = column.data_blocks.len().checked_sub(1) else {
        return Ok(false);
    };
    let (block, _) = segment
        .block(context, BlockKind::Data, 0, last_index)
        .await?;
    let Some(last_row) = block.row_count().checked_sub(1) else {
        return Ok(false);
    };
    Ok(block.get(last_row) >= key)
}
