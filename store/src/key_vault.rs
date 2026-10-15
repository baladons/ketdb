use std::collections::HashMap;

pub struct KeyVault {
    inner: HashMap<u32, Vec<u8>>,
}

impl KeyVault {
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
        }
    }

    pub fn insert(&mut self, key_id: u32, key: Vec<u8>) {
        self.inner.insert(key_id, key);
    }

    pub fn get(&self, key_id: u32) -> Option<&[u8]> {
        self.inner.get(&key_id).map(|key| key.as_slice())
    }
}