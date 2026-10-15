use compio::io::{AsyncReadAtExt, AsyncWriteAtExt};
use uuid::Uuid;

use crate::{
    aligned_vec::{page_align_down, page_align_up, AlignedVec},
    context::Context,
};

pub struct VirtualFile {
    path: String,
}

impl VirtualFile {
    pub fn new(id: Uuid) -> Self {
        Self {
            path: format!("data/{id}.ket"),
        }
    }

    pub fn mask(id: Uuid) -> Self {
        let file = Self {
            path: format!("data/{id}.mask.ket"),
        };
        file.ensure_exists();
        file
    }

    pub fn metadata() -> Self {
        let file = Self {
            path: "data/metadata.ket".to_string(),
        };
        file.ensure_exists();
        file
    }

    pub fn ensure(id: Uuid) -> Self {
        let file = Self::new(id);
        file.ensure_exists();
        file
    }

    pub fn remove(id: Uuid) {
        let _ = std::fs::remove_file(format!("data/{id}.ket"));
        let _ = std::fs::remove_file(format!("data/{id}.mask.ket"));
    }

    pub fn remove_metadata() {
        let _ = std::fs::remove_file("data/metadata.ket");
        crate::context::Context::clear_metadata();
    }

    fn ensure_exists(&self) {
        if let Some(parent) = std::path::Path::new(&self.path).parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        if !std::path::Path::new(&self.path).exists() {
            std::fs::File::create(&self.path).unwrap();
        }
    }

    pub async fn write_at(&self, offset: u64, buffer: &[u8]) {
        let mut file = Context::current().file_cache.get(&self.path).await;
        let io_buffer = AlignedVec::from_slice(buffer);
        file.write_all_at(io_buffer, offset).await.unwrap();
    }

    pub async fn read(&self, len: usize, offset: usize) -> Vec<u8> {
        let page_start = page_align_down(offset);
        let io_end = page_align_up(offset + len);
        let io_len = io_end - page_start;

        let buffer = AlignedVec::with_io_capacity(io_len);
        let file = Context::current().file_cache.get(&self.path).await;
        file.read_exact_at(buffer, page_start as u64)
            .await
            .unwrap()
            .1
            .slice_range(offset - page_start, len)
            .to_vec()
    }

    pub async fn len(&self) -> usize {
        let file = Context::current().file_cache.get(&self.path).await;
        file.metadata().await.unwrap().len() as usize
    }

    pub async fn append(&self, buffer: &[u8]) {
        let mut file = Context::current().file_cache.get(&self.path).await;
        let offset = file.metadata().await.unwrap().len();
        let io_buffer = AlignedVec::from_slice(buffer);
        file.write_all_at(io_buffer, offset)
            .await
            .unwrap();
    }
}
