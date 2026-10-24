use std::{collections::HashMap, io};

use compio::{
    BufResult,
    io::{AsyncReadAt, AsyncWriteAtExt},
};

use crate::{
    buffer::PAGE_SIZE,
    dtos::{Encoding, Frame},
    error::Error,
    file_id::FileId,
};

use super::{buffer_pool::BufferPool, file_manager::FileManager};

pub struct LastFrameManager {
    cache: HashMap<FileId, Frame>,
}

impl LastFrameManager {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    pub async fn read_last_frame(
        &mut self,
        pool: &mut BufferPool,
        files: &mut FileManager,
        id: FileId,
    ) -> Result<Frame, Error> {
        if let Some(frame) = self.cache.get(&id) {
            return Ok(frame.clone());
        }

        let file_len = files.file_len(id.clone()).await?;
        let page_offset = last_frame_page_offset(file_len);

        let mut buffer = pool.get(PAGE_SIZE);
        buffer.before_read(PAGE_SIZE);

        let file = files.get_file(id.clone()).await?;
        let BufResult(result, mut buffer) = file.read_at(buffer, page_offset).await;
        let n = result.map_err(Error::IoError)?;
        if n < 8 {
            return Err(Error::IoError(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "last page too short",
            )));
        }

        let read_len = n.min(12);
        buffer.after_read(read_len);

        let view: &[u32] = bytemuck::cast_slice(&buffer[..read_len]);
        let offset = view[0] as u64;
        let len = view[1];
        let uncompressed_size = if read_len >= 12 {
            view[2]
        } else {
            len
        };

        pool.recycle(buffer);

        let frame = Frame {
            encoding: Encoding::plain(),
            uncompressed_size,
            crc: 0,
            offset,
            len,
        };
        self.cache.insert(id, frame.clone());

        Ok(frame)
    }

    pub async fn write_last_frame(
        &mut self,
        pool: &mut BufferPool,
        files: &mut FileManager,
        id: FileId,
        frame: Frame,
    ) -> Result<(), Error> {
        let file_len = files.file_len(id.clone()).await?;
        let page_offset = last_frame_page_offset(file_len);

        let mut buffer = pool.get(PAGE_SIZE);
        write_last_frame_page_bytes(&mut buffer, &frame);
        buffer.before_write();

        let mut file = files.get_file(id.clone()).await?;
        let BufResult(result, buffer) = file.write_all_at(buffer, page_offset).await;
        result.map_err(Error::IoError)?;

        pool.recycle(buffer);

        self.cache.insert(id, frame);

        Ok(())
    }

    pub fn cache_last_frame(&mut self, id: FileId, frame: Frame) {
        self.cache.insert(id, frame);
    }
}

pub fn write_last_frame_page_bytes(buffer: &mut crate::buffer::Buffer, frame: &Frame) {
    buffer.extend_from_slice(&(frame.offset as u32).to_le_bytes());
    buffer.extend_from_slice(&(frame.len as u32).to_le_bytes());
    buffer.extend_from_slice(&frame.uncompressed_size.to_le_bytes());
}

pub fn last_page_offset(file_len: u64) -> u64 {
    if file_len == 0 {
        0
    } else {
        (file_len - 1) & !(PAGE_SIZE as u64 - 1)
    }
}

/// Page offset for the segment last-frame pointer (always appended, never overwrites payload).
pub fn last_frame_page_offset(file_len: u64) -> u64 {
    file_len.div_ceil(PAGE_SIZE as u64) * PAGE_SIZE as u64
}
