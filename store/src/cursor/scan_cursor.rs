use crate::{
    batch::Batch,
    block::Block,
    error::Error,
    file_id::FileId,
    segment::{BlockKind, Segment},
    storage::Storage,
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
        let segment = open_segment_at(&file_ids, 0, storage).await?;
        let segment_cursor =
            SegmentCursor::seek_first(segment, column_ids.clone(), storage).await?;
        Ok(Self {
            file_ids,
            segment_index: 0,
            column_ids,
            segment_cursor,
        })
    }

    pub async fn seek_key(
        file_ids: Vec<FileId>,
        column_ids: Vec<u32>,
        key: &[u8],
        storage: &mut Storage,
    ) -> Result<Self, Error> {
        for segment_index in 0..file_ids.len() {
            let segment = open_segment_at(&file_ids, segment_index, storage).await?;
            if !segment_contains_key(&segment, storage, key).await? {
                continue;
            }
            let segment_cursor =
                SegmentCursor::seek_key(segment, column_ids.clone(), key, storage).await?;
            return Ok(Self {
                file_ids,
                segment_index,
                column_ids,
                segment_cursor,
            });
        }

        Err(Error::DeserializationFailure)
    }

    /// Advance by `count` rows, opening the next segment when the current one is exhausted.
    /// Returns `false` if any step cannot be advanced (including past the last row).
    pub async fn advance(&mut self, storage: &mut Storage, count: u32) -> Result<bool, Error> {
        for _ in 0..count {
            if self.segment_cursor.advance(storage, 1).await? {
                continue;
            }

            let next_segment = self.segment_index + 1;
            if next_segment >= self.file_ids.len() {
                return Ok(false);
            }

            let segment = open_segment_at(&self.file_ids, next_segment, storage).await?;
            self.segment_cursor =
                SegmentCursor::seek_first(segment, self.column_ids.clone(), storage).await?;
            self.segment_index = next_segment;
        }
        Ok(true)
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

async fn open_segment_at(
    file_ids: &[FileId],
    index: usize,
    storage: &mut Storage,
) -> Result<Segment, Error> {
    let file_id = file_ids.get(index).ok_or(Error::DeserializationFailure)?;
    Segment::open(storage, *file_id).await
}

async fn segment_contains_key(
    segment: &Segment,
    storage: &mut Storage,
    key: &[u8],
) -> Result<bool, Error> {
    let column = segment.column(0).ok_or(Error::DeserializationFailure)?;
    if column.data_blocks.is_empty() {
        return Ok(false);
    }

    let last_handle_index = column.data_blocks.len() - 1;
    let (first_block, _) = segment.block(storage, BlockKind::Data, 0, 0).await?;
    let (last_block, _) = segment
        .block(storage, BlockKind::Data, 0, last_handle_index)
        .await?;

    let first_key = first_block.get(0);
    let last_key = last_block.get(last_block.len() - 1);

    Ok(key >= first_key && key <= last_key)
}
