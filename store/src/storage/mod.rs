mod buffer_manager;
mod buffer_pool;
mod encoder;
mod encoding_utils;
mod file_manager;
mod key_manager;
mod last_frame_manager;

pub use buffer_manager::BufferManager;
pub use buffer_pool::BufferPool;
pub use encoder::Encoder;
pub use file_manager::FileManager;
pub use key_manager::{KeyEntry, KeyManager};
pub use last_frame_manager::{
    LastFrameManager, last_frame_page_offset, last_page_offset, write_last_frame_page_bytes,
};
use rkyv::deserialize;

use std::{rc::Rc, time::Duration};

use crate::{
    block::Block, buffer::Buffer, dtos::{ArchivedBlockHandle, ArchivedFrame, Encoding, Frame}, error::Error, file_id::FileId,
};

pub struct Storage {
    buffer_manager: BufferManager,
    buffer_pool: BufferPool,
    encoder: Encoder,
    last_frame_manager: LastFrameManager,
    key_manager: KeyManager,
    file_manager: FileManager,
}

impl Storage {
    pub fn new(n_files: u64, n_bytes: u64) -> Self {
        Self {
            buffer_manager: BufferManager::new(n_bytes),
            buffer_pool: BufferPool::new(),
            encoder: Encoder::new(),
            last_frame_manager: LastFrameManager::new(),
            key_manager: KeyManager::new(),
            file_manager: FileManager::new(n_files),
        }
    }

    pub async fn block_from_handle(
        &mut self,
        file_id: FileId,
        handle: &ArchivedBlockHandle,
    ) -> Result<Block, Error> {
        let frame = deserialize::<_, ArchivedFrame>(&handle.frame)
            .map_err(|_| Error::DeserializationFailure)?;
        self.read_block(
            file_id,
            frame,
            handle.footer_offset.to_native() as usize,
            true,
        )
        .await
    }

    pub async fn read_block(
        &mut self,
        id: FileId,
        frame: Frame,
        footer_offset: usize,
        check_crc: bool,
    ) -> Result<Block, Error> {
        let buffer = self.read(id, frame, check_crc).await?.clone();
        Block::new(buffer, footer_offset)
    }

    pub async fn read(
        &mut self,
        id: FileId,
        frame: Frame,
        check_crc: bool,
    ) -> Result<&Rc<Buffer>, Error> {
        self.buffer_manager
            .read(
                &mut self.buffer_pool,
                &self.encoder,
                &mut self.file_manager,
                &mut self.key_manager,
                id,
                frame,
                check_crc,
            )
            .await
    }

    pub async fn write(
        &mut self,
        id: FileId,
        offset: u64,
        buffer: &Buffer,
        encoding: Encoding,
    ) -> Result<Frame, Error> {
        self.buffer_manager
            .write(
                &mut self.buffer_pool,
                &self.encoder,
                &mut self.file_manager,
                &mut self.key_manager,
                id,
                offset,
                buffer,
                encoding,
            )
            .await
    }

    pub async fn read_last_frame(&mut self, id: FileId) -> Result<Frame, Error> {
        self.last_frame_manager
            .read_last_frame(&mut self.buffer_pool, &mut self.file_manager, id)
            .await
    }

    pub async fn write_last_frame(&mut self, id: FileId, frame: Frame) -> Result<(), Error> {
        self.last_frame_manager
            .write_last_frame(&mut self.buffer_pool, &mut self.file_manager, id, frame)
            .await
    }

    pub fn insert_key(&mut self, file_id: FileId, key: Vec<u8>) {
        self.key_manager.insert(file_id, key);
    }

    pub fn cleanup_keys(&mut self, max_age: Duration) {
        self.key_manager.cleanup(max_age);
    }

    pub(crate) async fn encode_buffer(
        &mut self,
        id: FileId,
        buffer: &Buffer,
        encoding: Encoding,
    ) -> Result<Buffer, Error> {
        self.encoder
            .encode(
                &mut self.buffer_pool,
                &mut self.key_manager,
                id,
                buffer,
                encoding,
            )
            .await
    }

    pub(crate) fn recycle_encoded_buffer(&mut self, buffer: Buffer) {
        self.buffer_pool.recycle(buffer);
    }

    pub(crate) fn buffer_pool_mut(&mut self) -> &mut BufferPool {
        &mut self.buffer_pool
    }

    pub(crate) fn invalidate_cached_frame(&mut self, id: FileId, offset: u64) {
        self.buffer_manager.invalidate(id, offset);
    }

    pub(crate) fn cache_last_frame(&mut self, id: FileId, frame: Frame) {
        self.last_frame_manager.cache_last_frame(id, frame);
    }
}
