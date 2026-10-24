use compio::{
    BufResult,
    fs::File,
    io::AsyncWriteAtExt,
};

use crate::{
    buffer::{Buffer, PAGE_SIZE},
    dtos::{Encoding, Frame},
    error::Error,
    file_id::FileId,
    last_frame_manager::{last_frame_page_offset, write_last_frame_page_bytes},
    storage::Storage,
};

pub struct FrameWriter {
    file_id: FileId,
    file: File,
    offset: u64,
}

impl FrameWriter {
    pub async fn open(file_id: FileId) -> Result<Self, Error> {
        let file = file_id.open_buffered().await?;
        Ok(Self {
            file_id,
            file,
            offset: 0,
        })
    }

    pub fn file_id(&self) -> FileId {
        self.file_id
    }

    pub fn offset(&self) -> u64 {
        self.offset
    }

    pub async fn write(
        &mut self,
        storage: &mut Storage,
        encoding: Encoding,
        buffer: Buffer,
    ) -> Result<Frame, Error> {
        debug_assert!(self.offset % PAGE_SIZE as u64 == 0);

        let uncompressed_size = buffer.len() as u32;
        let mut encoded = storage
            .encode_buffer(self.file_id, &buffer, encoding)
            .await?;

        let len = encoded.len() as u32;
        let crc = crc32c::crc32c(&encoded);
        encoded.before_write();

        let offset = self.offset;
        let BufResult(result, encoded) = self.file.write_all_at(encoded, offset).await;
        result.map_err(Error::IoError)?;
        storage.recycle_encoded_buffer(encoded);

        self.offset = (offset + len as u64).next_multiple_of(PAGE_SIZE as u64);

        storage.invalidate_cached_frame(self.file_id, offset);

        Ok(Frame {
            encoding,
            uncompressed_size,
            crc,
            offset,
            len,
        })
    }

    pub async fn finish(&mut self, storage: &mut Storage, buffer: Buffer) -> Result<(), Error> {
        let frame = self.write(storage, Encoding::plain(), buffer).await?;
        self.write_last_frame_page(storage, frame).await
    }

    async fn write_last_frame_page(
        &mut self,
        storage: &mut Storage,
        frame: Frame,
    ) -> Result<(), Error> {
        let file_len = self.file.metadata().await.map_err(Error::IoError)?.len();
        let page_offset = last_frame_page_offset(file_len);

        let pool = storage.buffer_pool_mut();
        let mut page = pool.get(PAGE_SIZE);
        write_last_frame_page_bytes(&mut page, &frame);
        page.before_write();

        let BufResult(result, page) = self.file.write_all_at(page, page_offset).await;
        result.map_err(Error::IoError)?;
        pool.recycle(page);

        storage.cache_last_frame(self.file_id, frame);

        Ok(())
    }
}
