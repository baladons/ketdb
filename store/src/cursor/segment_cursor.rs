use std::cmp::Ordering;

use crate::{batch::Batch, block::Block, context::Context, error::Error, segment::Segment};

use super::block_cursor::BlockCursor;

pub struct SegmentCursor {
    segment: Segment,
    column_ids: Vec<u32>,
    cursors: Vec<BlockCursor>,
}

impl SegmentCursor {
    pub async fn seek_first(
        segment: Segment,
        column_ids: Vec<u32>,
        context: &mut Context,
    ) -> Result<Self, Error> {
        Self::at_row_index(segment, column_ids, context, 0).await
    }

    pub async fn seek_key(
        segment: Segment,
        column_ids: Vec<u32>,
        key: &[u8],
        context: &mut Context,
    ) -> Result<Self, Error> {
        let row_index = segment.row_index_from_key(context, key).await?.index();
        Self::at_row_index(segment, column_ids, context, row_index).await
    }

    pub async fn advance(&mut self, context: &mut Context, count: u32) -> Result<u32, Error> {
        let mut remaining = 0u32;

        for cursor in &mut self.cursors {
            let new_remaining = cursor.advance(&self.segment, context, count).await?;
            debug_assert!(remaining == 0 || remaining == new_remaining);
            remaining = new_remaining;
        }

        Ok(remaining)
    }

    pub fn segment(&self) -> &Segment {
        &self.segment
    }

    pub fn column_ids(&self) -> &[u32] {
        &self.column_ids
    }

    pub fn row_index(&self) -> u32 {
        self.cursors
            .first()
            .map(BlockCursor::row_index)
            .unwrap_or(0)
    }

    pub fn done(&self) -> bool {
        self.cursors.iter().any(BlockCursor::done)
    }

    pub fn block_cursors(&self) -> &[BlockCursor] {
        &self.cursors
    }

    pub fn blocks(&self) -> Vec<&Block> {
        self.cursors.iter().map(BlockCursor::block).collect()
    }

    pub fn batch(&self) -> Vec<Batch<'_>> {
        let batches = self
            .cursors
            .iter()
            .map(BlockCursor::batch)
            .collect::<Vec<_>>();

        let min_len = batches.iter().map(Batch::len).min().unwrap_or(0);
        batches
            .into_iter()
            .map(|batch| batch.truncate(min_len))
            .collect()
    }

    async fn at_row_index(
        segment: Segment,
        column_ids: Vec<u32>,
        context: &mut Context,
        row_index: u32,
    ) -> Result<Self, Error> {
        let mut cursors = Vec::with_capacity(column_ids.len());

        for column_id in &column_ids {
            cursors
                .push(BlockCursor::at_row_index(&segment, *column_id, row_index, context).await?);
        }

        Ok(Self {
            segment,
            column_ids,
            cursors,
        })
    }

    fn key(&self) -> &[u8] {
        let cursor = self
            .cursors
            .iter()
            .find(|cursor| cursor.column_id() == 0)
            .expect("column 0");
        cursor.block().get(cursor.index_in_block())
    }
}

/// Ordered by the column 0 key, smallest first.
impl PartialEq for SegmentCursor {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for SegmentCursor {}

impl PartialOrd for SegmentCursor {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SegmentCursor {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key().cmp(other.key())
    }
}
