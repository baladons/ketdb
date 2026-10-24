use crate::{batch::Batch, block::Block, error::Error, segment::{BlockKind, Segment}, storage::Storage};

pub struct Cursor {
    segment: Segment,
    column_ids: Vec<u32>,
    row_index: u32,
    blocks: Vec<Block>,
}

impl Cursor {
    pub async fn seek_first(
        segment: Segment,
        column_ids: Vec<u32>,
        storage: &mut Storage,
    ) -> Result<Self, Error> {
        let row_index = first_row_index(&segment, &column_ids)?;
        Self::at_row_index(segment, column_ids, storage, row_index).await
    }

    pub async fn seek_key(
        segment: Segment,
        column_ids: Vec<u32>,
        key: &[u8],
        storage: &mut Storage,
    ) -> Result<Self, Error> {
        let row_index = segment.search(storage, key).await?;
        Self::at_row_index(segment, column_ids, storage, row_index).await
    }

    pub fn last_row_index(segment: &Segment, column_ids: &[u32]) -> Result<u32, Error> {
        last_row_index(segment, column_ids)
    }

    pub fn row_index_exists(segment: &Segment, column_ids: &[u32], row_index: u32) -> bool {
        column_ids.iter().all(|&column_id| {
            segment
                .data_block_index_with_row_index(column_id, row_index)
                .is_ok()
        })
    }

    pub async fn load_at_row_index(
        &mut self,
        storage: &mut Storage,
        row_index: u32,
    ) -> Result<(), Error> {
        if !Self::row_index_exists(&self.segment, &self.column_ids, row_index) {
            return Err(Error::DeserializationFailure);
        }
        self.blocks =
            load_blocks_at_row(&self.segment, storage, &self.column_ids, row_index).await?;
        self.row_index = row_index;
        Ok(())
    }

    pub fn segment(&self) -> &Segment {
        &self.segment
    }

    pub fn column_ids(&self) -> &[u32] {
        &self.column_ids
    }

    pub fn row_index(&self) -> u32 {
        self.row_index
    }

    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    pub fn batch_read(&self) -> Vec<Batch<'_>> {
        let batches = self
            .blocks
            .iter()
            .map(|block| block.full_batch())
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
        let blocks = load_blocks_at_row(&segment, storage, &column_ids, row_index).await?;
        Ok(Self {
            segment,
            column_ids,
            row_index,
            blocks,
        })
    }
}

fn first_row_index(segment: &Segment, column_ids: &[u32]) -> Result<u32, Error> {
    let column_id = column_ids.first().ok_or(Error::DeserializationFailure)?;
    let column = segment
        .column(*column_id)
        .ok_or(Error::DeserializationFailure)?;
    let handle = column
        .data_blocks
        .first()
        .ok_or(Error::DeserializationFailure)?;
    Ok(handle.row_offset.to_native())
}

fn last_row_index(segment: &Segment, column_ids: &[u32]) -> Result<u32, Error> {
    let column_id = column_ids.first().ok_or(Error::DeserializationFailure)?;
    let column = segment
        .column(*column_id)
        .ok_or(Error::DeserializationFailure)?;
    let handle = column
        .data_blocks
        .last()
        .ok_or(Error::DeserializationFailure)?;
    Ok(handle.row_offset.to_native() + handle.row_len.to_native() - 1)
}

async fn load_blocks_at_row(
    segment: &Segment,
    storage: &mut Storage,
    column_ids: &[u32],
    row_index: u32,
) -> Result<Vec<Block>, Error> {
    let mut blocks = Vec::with_capacity(column_ids.len());
    for &column_id in column_ids {
        let data_block_index = segment.data_block_index_with_row_index(column_id, row_index)?;
        let block = segment
            .block(
                storage,
                BlockKind::Data,
                column_id,
                data_block_index,
            )
            .await?;
        blocks.push(block);
    }
    Ok(blocks)
}
