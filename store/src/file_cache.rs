use std::num::NonZero;

use compio::fs::{File, OpenOptions};
use lru::LruCache;
use uuid::Uuid;

pub struct FileCache {
    inner: LruCache<Uuid, File>,
}

impl FileCache {
    pub fn new(capacity: usize) -> Self {
        FileCache {
            inner: LruCache::new(NonZero::new(capacity).unwrap()).into(),
        }
    }

    pub async fn get(&mut self, id: Uuid) -> File {
        if let Some(file) = self.inner.get(&id) {
            return file.clone();
        }

        let path = format!("data/{}.ket", id);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .custom_flags(libc::O_DIRECT)
            .open(&path)
            .await
            .unwrap();
        self.inner.put(id, file.clone());
        file
    }
}
