use crate::{
    batch::Batch,
    block::Block,
    cursor::Cursor,
    error::Error,
    file_id::FileId,
    segment::{BlockKind, Segment},
    storage::Storage,
};

pub struct SequentialCursor {
    file_ids: Vec<FileId>,
    segment_index: usize,
    column_ids: Vec<u32>,
    cursor: Cursor,
}

impl SequentialCursor {
    pub async fn seek_first(
        file_ids: Vec<FileId>,
        column_ids: Vec<u32>,
        storage: &mut Storage,
    ) -> Result<Self, Error> {
        let segment = open_segment_at(&file_ids, 0, storage).await?;
        let cursor = Cursor::seek_first(segment, column_ids.clone(), storage).await?;
        Ok(Self {
            file_ids,
            segment_index: 0,
            column_ids,
            cursor,
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
            let cursor = Cursor::seek_key(segment, column_ids.clone(), key, storage).await?;
            return Ok(Self {
                file_ids,
                segment_index,
                column_ids,
                cursor,
            });
        }

        Err(Error::DeserializationFailure)
    }

    /// Advance to the next data block, opening the next segment when the current one is exhausted.
    /// Returns `false` when there is no further position.
    pub async fn advance(&mut self, storage: &mut Storage) -> Result<bool, Error> {
        let next_row = self.cursor.row_index() + 1;
        if Cursor::row_index_exists(self.cursor.segment(), &self.column_ids, next_row) {
            self.cursor.load_at_row_index(storage, next_row).await?;
            return Ok(true);
        }

        let next_segment = self.segment_index + 1;
        if next_segment >= self.file_ids.len() {
            return Ok(false);
        }

        let segment = open_segment_at(&self.file_ids, next_segment, storage).await?;
        self.cursor = Cursor::seek_first(segment, self.column_ids.clone(), storage).await?;
        self.segment_index = next_segment;
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
        self.cursor.segment()
    }

    pub fn column_ids(&self) -> &[u32] {
        &self.column_ids
    }

    pub fn row_index(&self) -> u32 {
        self.cursor.row_index()
    }

    pub fn blocks(&self) -> &[Block] {
        self.cursor.blocks()
    }

    pub fn batch_read(&self) -> Vec<Batch<'_>> {
        self.cursor.batch_read()
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
    let first_block = segment.block(storage, BlockKind::Data, 0, 0).await?;
    let last_block = segment
        .block(storage, BlockKind::Data, 0, last_handle_index)
        .await?;

    let first_key = first_block.get(0);
    let last_key = last_block.get(last_block.len() - 1);

    Ok(key >= first_key && key <= last_key)
}
