use std::rc::Rc;

use compio::{
    BufResult,
    io::{AsyncReadAt, AsyncWriteAtExt},
};
use mini_moka::unsync::Cache;

use crate::{
    buffer::{Buffer, PAGE_SIZE},
    dtos::{Encoding, Frame},
    encoding_utils::{compress_into, decompress_into, decrypt_in_place, encrypt_in_place},
    error::Error,
    file_id::FileId,
    file_manager::FileManager,
    key_manager::KeyManager,
};

pub struct FrameManager {
    temp_buffers: Vec<Buffer>,
    buffer_cache: Cache<(FileId, u64), Rc<Buffer>>,
}

impl FrameManager {
    pub fn new(n_bytes: u64) -> Self {
        Self {
            temp_buffers: Vec::new(),
            buffer_cache: Cache::<(FileId, u64), Rc<Buffer>>::builder()
                .max_capacity(n_bytes)
                .weigher(|_, v| v.capacity() as u32)
                .build(),
        }
    }

    pub async fn read(
        &mut self,
        files: &mut FileManager,
        keys: &mut KeyManager,
        id: FileId,
        frame: Frame,
        check_crc: bool,
    ) -> Result<&Rc<Buffer>, Error> {
        if !self.buffer_cache.contains_key(&(id.clone(), frame.offset)) {
            let mut temp_buffer = self.get_temp_buffer(frame.len as usize);

            temp_buffer.before_read(frame.len as usize);

            let file = files.get_file(id.clone()).await?;
            let BufResult(result, mut temp_buffer) = file.read_at(temp_buffer, frame.offset).await;
            result.map_err(Error::IoError)?;

            temp_buffer.after_read(frame.len as usize);

            if check_crc {
                assert_eq!(frame.crc, crc32c::crc32c(&temp_buffer), "crc mismatch");
            }

            if frame.encoding.encrypted {
                let key = keys.get_encryption_key(id.clone()).await;
                decrypt_in_place(key, &mut temp_buffer)?;
            }

            let mut buffer = Buffer::with_capacity(frame.uncompressed_size as usize);
            buffer.resize(frame.uncompressed_size as usize, 0);

            if frame.encoding.compression > 0 {
                decompress_into(&temp_buffer, &mut buffer)?;
            } else {
                buffer.extend_from_slice(&temp_buffer);
            }

            self.temp_buffers.push(temp_buffer);

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
        files: &mut FileManager,
        keys: &mut KeyManager,
        id: FileId,
        offset: u64,
        buffer: &Buffer,
        encoding: Encoding,
    ) -> Result<Frame, Error> {
        debug_assert!(offset % PAGE_SIZE as u64 == 0);

        let mut temp_buffer = self.get_temp_buffer(buffer.len() / 2);
        temp_buffer.clear();

        if encoding.compression > 0 {
            compress_into(encoding.compression as i32, &buffer, &mut temp_buffer)?;
        } else {
            temp_buffer.extend_from_slice(&buffer);
        }

        if encoding.encrypted {
            let key = keys.get_encryption_key(id.clone()).await;
            encrypt_in_place(key, &mut temp_buffer)?;
        }

        let mut file = files.get_file(id.clone()).await?;

        let len = temp_buffer.len() as u64;
        let crc = crc32c::crc32c(&temp_buffer);

        temp_buffer.before_write();

        let BufResult(result, temp_buffer) = file.write_all_at(temp_buffer, offset).await;
        result.map_err(Error::IoError)?;

        self.temp_buffers.push(temp_buffer);

        self.buffer_cache.invalidate(&(id.clone(), offset));

        Ok(Frame {
            encoding,
            uncompressed_size: buffer.len() as u32,
            crc,
            offset,
            len,
        })
    }

    fn get_temp_buffer(&mut self, size: usize) -> Buffer {
        self.temp_buffers
            .pop()
            .unwrap_or_else(|| Buffer::with_capacity(size))
    }
}
