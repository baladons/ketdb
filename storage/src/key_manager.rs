use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use secrecy::{ExposeSecret, SecretSlice};

use crate::file_id::FileId;

pub struct KeyEntry {
    key: SecretSlice<u8>,
    last_access: Instant,
}

pub struct KeyManager {
    keys: HashMap<FileId, KeyEntry>,
}

impl KeyManager {
    pub fn new() -> Self {
        Self {
            keys: HashMap::new(),
        }
    }

    pub fn insert(&mut self, file_id: FileId, key: Vec<u8>) {
        let entry = KeyEntry {
            key: SecretSlice::new(key.into_boxed_slice()),
            last_access: Instant::now(),
        };
        self.keys.insert(file_id, entry);
    }

    pub fn get(&mut self, id: &FileId) -> Option<&[u8]> {
        self.keys.get_mut(id).map(|entry| {
            entry.last_access = Instant::now();
            entry.key.expose_secret()
        })
    }

    pub fn cleanup(&mut self, max_age: Duration) {
        let now = Instant::now();
        self.keys
            .retain(|_, entry| now.duration_since(entry.last_access) <= max_age);
    }

    pub async fn get_encryption_key(&mut self, file_id: FileId) -> &[u8] {
        let _ = file_id;
        unimplemented!()
    }
}
