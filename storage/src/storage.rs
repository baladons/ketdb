use std::{rc::Rc, time::Duration};

use crate::{
    buffer::Buffer,
    dtos::{Encoding, Frame},
    error::Error,
    file_id::FileId,
    file_manager::FileManager,
    frame_manager::FrameManager,
    key_manager::KeyManager,
};

pub struct Storage {
    frame_manager: FrameManager,
    key_manager: KeyManager,
    file_manager: FileManager,
}

impl Storage {
    pub fn new(n_files: u64, n_bytes: u64) -> Self {
        Self {
            frame_manager: FrameManager::new(n_bytes),
            key_manager: KeyManager::new(),
            file_manager: FileManager::new(n_files),
        }
    }

    pub async fn read(
        &mut self,
        id: FileId,
        frame: Frame,
        check_crc: bool,
    ) -> Result<&Rc<Buffer>, Error> {
        self.frame_manager
            .read(
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
        self.frame_manager
            .write(
                &mut self.file_manager,
                &mut self.key_manager,
                id,
                offset,
                buffer,
                encoding,
            )
            .await
    }

    pub fn insert_key(&mut self, file_id: FileId, key: Vec<u8>) {
        self.key_manager.insert(file_id, key);
    }

    pub fn cleanup_keys(&mut self, max_age: Duration) {
        self.key_manager.cleanup(max_age);
    }
}

#[cfg(test)]
mod tests {
    use crate::{buffer::Buffer, dtos::Encoding, file_id::FileId, storage::Storage};

    #[compio::test]
    async fn test_write_read() {
        let mut storage = Storage::new(10, 1024 * 1024);

        let file_id = FileId::new_segment();
        let offset = 0;
        let data = b"Hello world!".repeat(10);
        let mut buffer = Buffer::new();
        buffer.extend_from_slice(&data);

        let frame = storage
            .write(file_id.clone(), offset, &buffer, Encoding::compressed())
            .await
            .expect("test");

        let read_buffer = storage
            .read(file_id.clone(), frame, true)
            .await
            .expect("test");

        assert_eq!(&read_buffer[..], data);
    }
}
