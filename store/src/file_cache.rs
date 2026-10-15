use std::num::NonZero;

use compio::fs::{File, OpenOptions};
use lru::LruCache;

pub struct FileCache {
    inner: LruCache<String, File>,
}

impl FileCache {
    pub fn new(capacity: usize) -> Self {
        FileCache {
            inner: LruCache::new(NonZero::new(capacity).unwrap()).into(),
        }
    }

    pub async fn get(&mut self, path: &str) -> File {
        if let Some(file) = self.inner.get(path) {
            return file.clone();
        }

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .custom_flags(libc::O_DIRECT)
            .open(path)
            .await
            .unwrap();
        self.inner.put(path.to_string(), file.clone());
        file
    }
}
