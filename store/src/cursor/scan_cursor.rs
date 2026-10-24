use crate::{
    batch::Batch, block::Block, context::Context, error::Error, file_id::FileId, segment::Segment,
};

use super::segment_cursor::SegmentCursor;

pub struct ScanCursor {
    file_ids: Vec<FileId>,
    segment_index: usize,
    column_ids: Vec<u32>,
    segment_cursor: SegmentCursor,
}

impl ScanCursor {
    pub async fn seek_first(
        file_ids: Vec<FileId>,
        column_ids: Vec<u32>,
        context: &mut Context,
    ) -> Result<Self, Error> {
        let file_id = file_ids.first().ok_or(Error::DeserializationFailure)?;
        let segment = Segment::open(context, *file_id).await?;
        let segment_cursor =
            SegmentCursor::seek_first(segment, column_ids.clone(), context).await?;
        Ok(Self {
            file_ids,
            segment_index: 0,
            column_ids,
            segment_cursor,
        })
    }

    pub async fn advance(&mut self, context: &mut Context, count: u32) -> Result<u32, Error> {
        let mut remaining = count;
        loop {
            remaining = self.segment_cursor.advance(context, remaining).await?;

            if remaining == 0 {
                return Ok(0);
            }

            self.segment_index += 1;

            if self.segment_index >= self.file_ids.len() {
                return Ok(remaining);
            }

            let file_id = self.file_ids[self.segment_index];
            let segment = Segment::open(context, file_id).await?;
            self.segment_cursor =
                SegmentCursor::seek_first(segment, self.column_ids.clone(), context).await?;
        }
    }

    pub fn segment_index(&self) -> usize {
        self.segment_index
    }

    pub fn current_file_id(&self) -> FileId {
        self.file_ids[self.segment_index]
    }

    pub fn column_ids(&self) -> &[u32] {
        &self.column_ids
    }

    pub fn row_index(&self) -> u32 {
        self.segment_cursor.row_index()
    }

    pub fn blocks(&self) -> Vec<&Block> {
        self.segment_cursor.blocks()
    }

    pub fn batch_read(&self) -> Vec<Batch<'_>> {
        self.segment_cursor.batch_read()
    }
}
