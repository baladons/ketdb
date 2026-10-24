use crate::{
    batch::Batch, block::Block, error::Error, file_id::FileId, segment::Segment, storage::Storage,
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
        storage: &mut Storage,
    ) -> Result<Self, Error> {
        let file_id = file_ids.first().ok_or(Error::DeserializationFailure)?;
        let segment = Segment::open(storage, *file_id).await?;
        let segment_cursor =
            SegmentCursor::seek_first(segment, column_ids.clone(), storage).await?;
        Ok(Self {
            file_ids,
            segment_index: 0,
            column_ids,
            segment_cursor,
        })
    }

    pub async fn advance(&mut self, storage: &mut Storage, count: u32) -> Result<u32, Error> {
        let mut remaining = count;
        while remaining > 0 {
            remaining = self.segment_cursor.advance(storage, remaining).await?;
            if remaining == 0 {
                return Ok(0);
            }

            let next_segment = self.segment_index + 1;
            if next_segment >= self.file_ids.len() {
                return Ok(remaining);
            }

            let file_id = self.file_ids[next_segment];
            let segment = Segment::open(storage, file_id).await?;
            self.segment_cursor =
                SegmentCursor::seek_first(segment, self.column_ids.clone(), storage).await?;
            self.segment_index = next_segment;
            remaining -= 1;
        }
        Ok(0)
    }

    pub fn file_ids(&self) -> &[FileId] {
        &self.file_ids
    }

    pub fn segment_index(&self) -> usize {
        self.segment_index
    }

    pub fn current_file_id(&self) -> FileId {
        self.file_ids[self.segment_index]
    }

    pub fn segment(&self) -> &Segment {
        self.segment_cursor.segment()
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
