use compio::io::{AsyncReadAtExt, AsyncWriteAtExt};
use uuid::Uuid;

use crate::context::Context;

pub struct VirtualFile {
    id: Uuid,
}

impl VirtualFile {
    pub fn new(id: Uuid) -> Self {
        VirtualFile { id }
    }

    pub async fn read(&self, buffer: Vec<u8>, offset: usize) -> Vec<u8> {
        let file = Context::current().file_cache.get(self.id).await;
        file.read_exact_at(buffer, offset as u64).await.unwrap().1
    }

    pub async fn len(&self) -> usize {
        let file = Context::current().file_cache.get(self.id).await;
        file.metadata().await.unwrap().len() as usize
    }

    pub async fn append(&self, buffer: &[u8]) -> Vec<u8> {
        let mut file = Context::current().file_cache.get(self.id).await;
        let offset = file.metadata().await.unwrap().len();
        file.write_all_at(buffer.to_vec(), offset).await.unwrap().1
    }
}
