use compio::fs::File;
use mini_moka::unsync::Cache;

use crate::{error::Error, file_id::FileId};

pub struct FileManager {
    file_cache: Cache<FileId, File>,
}

impl FileManager {
    pub fn new(n_files: u64) -> Self {
        Self {
            file_cache: Cache::new(n_files),
        }
    }

    pub async fn get_file(&mut self, id: FileId) -> Result<&File, Error> {
        if !self.file_cache.contains_key(&id) {
            let file = id.open(libc::O_DIRECT).await?;
            self.file_cache.insert(id.clone(), file);
        }

        Ok(self.file_cache.get(&id).expect("unreachable"))
    }

    pub async fn file_len(&mut self, id: FileId) -> Result<u64, Error> {
        let file = self.get_file(id).await?;
        Ok(file.metadata().await.map_err(Error::IoError)?.len())
    }
}
