use std::rc::Rc;

use rkyv::{access, access_unchecked};

use crate::{
    binary_search::{SearchResult, binary_search},
    block::Block,
    buffer::Buffer,
    context::Context,
    dtos::{ArchivedBlockHandle, ArchivedColumnHandle, ArchivedSegmentFooter},
    error::Error,
    file_id::FileId,
};

const KEY_COLUMN_ID: u32 = 0;

pub struct Segment {
    file_id: FileId,
    footer_buffer: Rc<Buffer>,
}

pub enum BlockKind {
    Data,
    Min,
    Max,
}

impl Segment {
    pub fn new(file_id: FileId, buffer: Rc<Buffer>) -> Result<Self, Error> {
        access::<ArchivedSegmentFooter, rkyv::rancor::Error>(&buffer)
            .map_err(|_| Error::DeserializationFailure)?;

        Ok(Self {
            file_id,
            footer_buffer: buffer,
        })
    }

    pub fn file_id(&self) -> FileId {
        self.file_id
    }

    pub async fn open(context: &mut Context, file_id: FileId) -> Result<Self, Error> {
        let frame = context.read_last_frame(file_id).await?;
        let buffer = context.read(file_id, frame, false).await?.clone();
        Self::new(file_id, buffer)
    }

    pub async fn row_index_from_key(
        &self,
        context: &mut Context,
        key: &[u8],
    ) -> Result<SearchResult, Error> {
        let data_block_index = self.data_block_index_from_key(context, key).await?;
        let column = self.key_column().ok_or(Error::DeserializationFailure)?;
        let row_offset = column.data_blocks[data_block_index].row_offset.to_native();

        let (data, _) = self
            .block(context, BlockKind::Data, KEY_COLUMN_ID, data_block_index)
            .await?;

        let index_in_data = data.search(key).await;

        Ok(index_in_data.map(|index| index + row_offset))
    }

    pub async fn data_block_index_from_key(
        &self,
        context: &mut Context,
        key: &[u8],
    ) -> Result<usize, Error> {
        let min_block_index = self.min_block_index_from_key(context, key).await?;
        let column = self.key_column().ok_or(Error::DeserializationFailure)?;
        let min_row_offset = column.min_blocks[min_block_index].row_offset.to_native();

        let (block, _) = self
            .block(context, BlockKind::Min, KEY_COLUMN_ID, min_block_index)
            .await?;

        let index_in_min = block.search(key).await;
        let mut index_in_min = index_in_min.index() as usize;
        if index_in_min == block.row_count() || block.get(index_in_min) > key {
            index_in_min = index_in_min.saturating_sub(1);
        }

        Ok(min_row_offset as usize + index_in_min)
    }

    pub async fn min_block_index_from_key(
        &self,
        context: &mut Context,
        key: &[u8],
    ) -> Result<usize, Error> {
        let column = self.key_column().ok_or(Error::DeserializationFailure)?;
        let min_handles = column.min_blocks.as_slice();
        if min_handles.is_empty() {
            return Err(Error::DeserializationFailure);
        }

        let index = binary_search(min_handles.len(), async |mid| {
            let (block, _) = self
                .block(context, BlockKind::Min, KEY_COLUMN_ID, mid)
                .await?;
            Ok(block.get(0).cmp(key))
        })
        .await?;

        Ok(match index {
            SearchResult::Found(index) => index as usize,
            SearchResult::NotFound(index) => index.saturating_sub(1) as usize,
        })
    }

    pub async fn block(
        &self,
        context: &mut Context,
        block_kind: BlockKind,
        column_id: u32,
        block_index: usize,
    ) -> Result<(Block, &ArchivedBlockHandle), Error> {
        let column = self
            .column(column_id)
            .ok_or(Error::DeserializationFailure)?;
        let blocks = match block_kind {
            BlockKind::Data => &column.data_blocks,
            BlockKind::Min => &column.min_blocks,
            BlockKind::Max => &column.max_blocks,
        };
        let handle = blocks
            .get(block_index)
            .ok_or(Error::DeserializationFailure)?;

        Ok((
            context.block_from_handle(self.file_id, handle).await?,
            handle,
        ))
    }

    pub fn column(&self, id: u32) -> Option<&ArchivedColumnHandle> {
        self.footer()
            .columns
            .iter()
            .find(|column| column.id.to_native() == id)
    }

    pub fn block_index_from_row_index(
        &self,
        column_id: u32,
        row_index: u32,
    ) -> Result<usize, Error> {
        let column = self
            .column(column_id)
            .ok_or(Error::DeserializationFailure)?;

        let handles = column.data_blocks.as_slice();

        let index = match handles
            .binary_search_by(|handle| handle.row_offset.to_native().cmp(&row_index))
        {
            Ok(index) => index,
            Err(index) => index.saturating_sub(1),
        };
        let handle = handles.get(index).ok_or(Error::DeserializationFailure)?;
        let start = handle.row_offset.to_native();
        let end = start + handle.row_len.to_native();
        if row_index < start || row_index >= end {
            Err(Error::DeserializationFailure)
        } else {
            Ok(index)
        }
    }

    fn key_column(&self) -> Option<&ArchivedColumnHandle> {
        self.column(KEY_COLUMN_ID)
    }

    fn footer(&self) -> &ArchivedSegmentFooter {
        // SAFETY: we already checked it in new
        unsafe { access_unchecked(&self.footer_buffer) }
    }
}
