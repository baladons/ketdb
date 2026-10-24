use crate::{batch::Batch, block::Block, error::Error, segment::Segment, storage::Storage};

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
        storage: &mut Storage,
    ) -> Result<Self, Error> {
        Self::at_row_index(segment, column_ids, storage, 0).await
    }

    pub async fn seek_key(
        segment: Segment,
        column_ids: Vec<u32>,
        key: &[u8],
        storage: &mut Storage,
    ) -> Result<Self, Error> {
        let row_index = segment.search_row_index(storage, key).await?.index();
        Self::at_row_index(segment, column_ids, storage, row_index).await
    }

    /// Returns how many rows of `count` could not be advanced (`0` if all succeeded).
    pub async fn advance(&mut self, storage: &mut Storage, count: u32) -> Result<u32, Error> {
        let mut remaining = 0u32;
        for cursor in &mut self.cursors {
            remaining = remaining.max(cursor.advance(&self.segment, storage, count).await?);
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

    pub fn block_cursors(&self) -> &[BlockCursor] {
        &self.cursors
    }

    pub fn blocks(&self) -> Vec<&Block> {
        self.cursors.iter().map(BlockCursor::block).collect()
    }

    pub fn batch_read(&self) -> Vec<Batch<'_>> {
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
        storage: &mut Storage,
        row_index: u32,
    ) -> Result<Self, Error> {
        let mut cursors = Vec::with_capacity(column_ids.len());

        for column_id in &column_ids {
            cursors.push(BlockCursor::at_row_index(&segment, *column_id, row_index, storage).await?);
        }

        Ok(Self {
            segment,
            column_ids,
            cursors,
        })
    }
}
