use std::rc::Rc;

use rkyv::{access, access_unchecked, deserialize};

use crate::{
    binary_search::{SearchResult, binary_search},
    block::Block,
    buffer::Buffer,
    dtos::{ArchivedBlockHandle, ArchivedColumnHandle, ArchivedFrame, ArchivedSegmentFooter},
    error::Error,
    file_id::FileId,
    storage::Storage,
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

    pub async fn open(storage: &mut Storage, file_id: FileId) -> Result<Self, Error> {
        let frame = storage.read_last_frame(file_id).await?;
        let buffer = storage.read(file_id, frame, false).await?.clone();
        Self::new(file_id, buffer)
    }

    pub async fn search(&self, storage: &mut Storage, key: &[u8]) -> Result<u32, Error> {
        let min_block_index = self.search_min_block_index(storage, key).await?;
        let index_in_min = self
            .search_index_in_min_block(storage, min_block_index, key)
            .await?;

        let column = self.key_column().ok_or(Error::DeserializationFailure)?;
        let start_data_block_index = column.min_blocks[min_block_index].row_offset.to_native();
        Ok(start_data_block_index + index_in_min as u32)
    }

    pub async fn search_min_block_index(
        &self,
        storage: &mut Storage,
        key: &[u8],
    ) -> Result<usize, Error> {
        let column = self.key_column().ok_or(Error::DeserializationFailure)?;
        let min_handles = column.min_blocks.as_slice();
        if min_handles.is_empty() {
            return Err(Error::DeserializationFailure);
        }

        let pick = binary_search(min_handles.len(), async |mid| {
            let block = self.min_block(storage, mid).await?;
            Ok(block.get(0).cmp(key))
        })
        .await?;

        Ok(match pick {
            SearchResult::Found(index) => index,
            SearchResult::NotFound(index) => index.saturating_sub(1),
        })
    }

    pub async fn search_index_in_min_block(
        &self,
        storage: &mut Storage,
        min_block_index: usize,
        key: &[u8],
    ) -> Result<usize, Error> {
        let block = self.min_block(storage, min_block_index).await?;
        let pick = block.search(key).await;
        let mut index = pick.index();
        if index == block.len() || block.get(index) > key {
            index = index.saturating_sub(1);
        }
        Ok(index)
    }

    pub async fn block(
        &self,
        storage: &mut Storage,
        block_kind: BlockKind,
        column_id: u32,
        block_index: usize,
    ) -> Result<Block, Error> {
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
        self.block_from_handle(storage, handle).await
    }

    async fn min_block(&self, storage: &mut Storage, index: usize) -> Result<Block, Error> {
        let column = self
            .column(KEY_COLUMN_ID)
            .ok_or(Error::DeserializationFailure)?;
        let handle = &column.min_blocks[index];
        self.block_from_handle(storage, handle).await
    }

    pub fn column(&self, id: u32) -> Option<&ArchivedColumnHandle> {
        self.footer()
            .columns
            .iter()
            .find(|column| column.id.to_native() == id)
    }

    pub fn data_block_index_with_row_index(
        &self,
        column_id: u32,
        row_index: u32,
    ) -> Result<usize, Error> {
        let column = self
            .column(column_id)
            .ok_or(Error::DeserializationFailure)?;

        let handles = column.data_blocks.as_slice();

        match handles.binary_search_by(|handle| handle.row_offset.to_native().cmp(&row_index)) {
            Ok(index) => Ok(index),
            Err(index) => Ok(index.saturating_sub(1)),
        }
    }

    fn key_column(&self) -> Option<&ArchivedColumnHandle> {
        self.column(KEY_COLUMN_ID)
    }

    fn footer(&self) -> &ArchivedSegmentFooter {
        // SAFETY: we already checked it in new
        unsafe { access_unchecked(&self.footer_buffer) }
    }

    async fn block_from_handle(
        &self,
        storage: &mut Storage,
        handle: &ArchivedBlockHandle,
    ) -> Result<Block, Error> {
        let frame = deserialize::<_, ArchivedFrame>(&handle.frame)
            .map_err(|_| Error::DeserializationFailure)?;
        storage
            .read_block(
                self.file_id,
                frame,
                handle.footer_offset.to_native() as usize,
                true,
            )
            .await
    }
}
