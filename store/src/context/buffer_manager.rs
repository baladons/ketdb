use std::rc::Rc;

use compio::{
    BufResult,
    io::{AsyncReadAt, AsyncWriteAtExt},
};
use mini_moka::unsync::Cache;

use crate::{
    buffer::{Buffer, PAGE_SIZE},
    dtos::{Encoding, Frame},
    error::Error,
    file_id::FileId,
};

use super::{
    buffer_pool::BufferPool, encoder::Encoder, file_manager::FileManager, key_manager::KeyManager,
};

pub struct BufferManager {
    buffer_cache: Cache<(FileId, u64), Rc<Buffer>>,
}

impl BufferManager {
    pub fn new(n_bytes: u64) -> Self {
        Self {
            buffer_cache: Cache::<(FileId, u64), Rc<Buffer>>::builder()
                .max_capacity(n_bytes)
                .weigher(|_, v| v.capacity() as u32)
                .build(),
        }
    }

    pub async fn read(
        &mut self,
        pool: &mut BufferPool,
        encoder: &Encoder,
        files: &mut FileManager,
        keys: &mut KeyManager,
        id: FileId,
        frame: Frame,
        check_crc: bool,
    ) -> Result<&Rc<Buffer>, Error> {
        if !self.buffer_cache.contains_key(&(id.clone(), frame.offset)) {
            let mut encoded = pool.get(frame.len as usize);
            encoded.before_read(frame.len as usize);

            let file = files.get_file(id.clone()).await?;
            let BufResult(result, mut encoded) = file.read_at(encoded, frame.offset).await;
            result.map_err(Error::IoError)?;

            encoded.after_read(frame.len as usize);

            if check_crc {
                assert_eq!(frame.crc, crc32c::crc32c(&encoded), "crc mismatch");
            }

            let buffer = encoder
                .decode(pool, keys, id.clone(), &frame, encoded)
                .await?;

            self.buffer_cache
                .insert((id.clone(), frame.offset), Rc::new(buffer));
        }

        Ok(self
            .buffer_cache
            .get(&(id, frame.offset))
            .expect("unreachable"))
    }

    pub async fn write(
        &mut self,
        pool: &mut BufferPool,
        encoder: &Encoder,
        files: &mut FileManager,
        keys: &mut KeyManager,
        id: FileId,
        offset: u64,
        buffer: &Buffer,
        encoding: Encoding,
    ) -> Result<Frame, Error> {
        debug_assert!(offset % PAGE_SIZE as u64 == 0);

        let mut encoded = encoder
            .encode(pool, keys, id.clone(), buffer, encoding)
            .await?;

        let mut file = files.get_file(id.clone()).await?;

        let len = encoded.len() as u32;
        let crc = crc32c::crc32c(&encoded);

        encoded.before_write();

        let BufResult(result, encoded) = file.write_all_at(encoded, offset).await;
        result.map_err(Error::IoError)?;

        pool.recycle(encoded);

        self.invalidate(id, offset);

        Ok(Frame {
            encoding,
            uncompressed_size: buffer.len() as u32,
            crc,
            offset,
            len,
        })
    }

    pub fn invalidate(&mut self, id: FileId, offset: u64) {
        self.buffer_cache.invalidate(&(id, offset));
    }
}
