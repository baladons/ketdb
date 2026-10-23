use std::{collections::HashMap, io};

use compio::{
    BufResult,
    io::{AsyncReadAt, AsyncWriteAtExt},
};

use crate::{
    buffer::{Buffer, PAGE_SIZE},
    dtos::{Encoding, Frame},
    error::Error,
    file_id::FileId,
    file_manager::FileManager,
};

pub struct LastFrameManager {
    cache: HashMap<FileId, Frame>,
    temp_buffers: Vec<Buffer>,
}

impl LastFrameManager {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            temp_buffers: Vec::new(),
        }
    }

    pub async fn read_last_frame(
        &mut self,
        files: &mut FileManager,
        id: FileId,
    ) -> Result<Frame, Error> {
        if let Some(frame) = self.cache.get(&id) {
            return Ok(frame.clone());
        }

        let file_len = files.file_len(id.clone()).await?;

        let mut buffer = self.get_temp_buffer(PAGE_SIZE);
        buffer.before_read(PAGE_SIZE);

        let file = files.get_file(id.clone()).await?;
        let BufResult(result, mut buffer) = file.read_at(buffer, last_page_offset(file_len)).await;
        let n = result.map_err(Error::IoError)?;
        if n < 8 {
            return Err(Error::IoError(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "last page too short",
            )));
        }

        buffer.after_read(8);

        let view: &[u32] = bytemuck::cast_slice(&buffer);
        let offset = view[0] as u64;
        let len = view[1];

        self.temp_buffers.push(buffer);

        let frame = Frame {
            encoding: Encoding::plain(),
            uncompressed_size: len,
            crc: 0,
            offset,
            len,
        };
        self.cache.insert(id, frame.clone());

        Ok(frame)
    }

    pub async fn write_last_frame(
        &mut self,
        files: &mut FileManager,
        id: FileId,
        frame: Frame,
    ) -> Result<(), Error> {
        let file_len = files.file_len(id.clone()).await?;
        let page_offset = last_page_offset(file_len);

        let mut buffer = self.get_temp_buffer(PAGE_SIZE);
        buffer.extend_from_slice(&(frame.offset as u32).to_le_bytes());
        buffer.extend_from_slice(&(frame.len as u32).to_le_bytes());
        buffer.before_write();

        let mut file = files.get_file(id.clone()).await?;
        let BufResult(result, buffer) = file.write_all_at(buffer, page_offset).await;
        result.map_err(Error::IoError)?;

        self.temp_buffers.push(buffer);

        let cached = Frame {
            encoding: Encoding::plain(),
            uncompressed_size: frame.len,
            crc: 0,
            offset: frame.offset,
            len: frame.len,
        };
        self.cache.insert(id, cached);

        Ok(())
    }

    fn get_temp_buffer(&mut self, size: usize) -> Buffer {
        let mut buffer = self
            .temp_buffers
            .pop()
            .unwrap_or_else(|| Buffer::with_capacity(size));

        buffer.clear();
        buffer
    }
}

fn last_page_offset(file_len: u64) -> u64 {
    if file_len == 0 {
        0
    } else {
        (file_len - 1) & !(PAGE_SIZE as u64 - 1)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        buffer::Buffer,
        buffer_manager::BufferManager,
        dtos::{Encoding, Frame},
        file_id::FileId,
        file_manager::FileManager,
        key_manager::KeyManager,
        last_frame_manager::LastFrameManager,
    };

    #[compio::test]
    async fn test_last_frame() {
        let mut buffers = BufferManager::new(1024 * 1024);
        let mut last_frames = LastFrameManager::new();
        let mut files = FileManager::new(10);
        let mut keys = KeyManager::new();
        let file_id = FileId::new_segment();

        let data = b"Hello world!".repeat(10);
        let mut buffer = Buffer::new();
        buffer.extend_from_slice(&data);
        let written = buffers
            .write(
                &mut files,
                &mut keys,
                file_id.clone(),
                0,
                &buffer,
                Encoding::compressed(),
            )
            .await
            .expect("test");
        buffers
            .write(
                &mut files,
                &mut keys,
                file_id.clone(),
                4096,
                &buffer,
                Encoding::compressed(),
            )
            .await
            .expect("test");

        let pointer = Frame {
            encoding: Encoding::compressed(),
            uncompressed_size: 1,
            crc: 7,
            offset: written.offset,
            len: written.len,
        };
        last_frames
            .write_last_frame(&mut files, file_id.clone(), pointer)
            .await
            .expect("test");

        let mut last_frames = LastFrameManager::new();
        let mut files = FileManager::new(10);
        let read = last_frames
            .read_last_frame(&mut files, file_id.clone())
            .await
            .expect("test");
        assert_eq!(read.offset, written.offset);
        assert_eq!(read.len, written.len);
        assert_eq!(read.uncompressed_size, written.len);
        assert_eq!(read.crc, 0);
        assert!(!read.encoding.encrypted);
        assert_eq!(read.encoding.compression, 0);

        let cached = last_frames
            .read_last_frame(&mut files, file_id.clone())
            .await
            .expect("test");
        assert_eq!(cached.offset, written.offset);

        let mut buffers = BufferManager::new(1024 * 1024);
        let mut keys = KeyManager::new();
        let read_buffer = buffers
            .read(&mut files, &mut keys, file_id, written, true)
            .await
            .expect("test");
        assert_eq!(&read_buffer[..], data);
    }
}
