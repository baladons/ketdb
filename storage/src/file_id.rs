use compio::fs::{File, OpenOptions};
use uuid::Uuid;

use crate::error::Error;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Copy)]
pub enum FileId {
    Segment(Uuid),
    Mask(Uuid),
    Metadata,
}

impl FileId {
    pub fn new_segment() -> Self {
        FileId::Segment(Uuid::now_v7())
    }

    pub fn new_mask() -> Self {
        FileId::Mask(Uuid::now_v7())
    }

    pub fn new_metadata() -> Self {
        FileId::Metadata
    }

    pub async fn open(&self, flags: i32) -> Result<File, Error> {
        let path = match &self {
            FileId::Segment(uuid) => format!("data/{}.seg", uuid),
            FileId::Mask(uuid) => format!("data/{}.mask", uuid),
            FileId::Metadata => "data/metadata".to_string(),
        };

        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .custom_flags(flags)
            .open(path)
            .await
            .map_err(Error::IoError)
    }
}
