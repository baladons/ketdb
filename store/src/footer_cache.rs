use std::{mem, rc::Rc};

use mini_moka::unsync::Cache;
use rkyv::{from_bytes_unchecked, rancor::Error};
use uuid::Uuid;

use crate::{footer::Footer, virtual_file::VirtualFile};

pub struct FooterCache {
    inner: Cache<Uuid, Rc<Footer>>,
}

impl FooterCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Cache::new(capacity as u64).into(),
        }
    }

    pub async fn get(&mut self, id: Uuid) -> Rc<Footer> {
        if let Some(table) = self.inner.get(&id) {
            return table.clone();
        }

        let file = VirtualFile::new(id);
        let len = file.len().await;
        let footer_offset_bytes = file
            .read(vec![0; mem::size_of::<u64>()], len - mem::size_of::<u64>())
            .await;
        let footer_offset = u64::from_le_bytes(footer_offset_bytes.try_into().unwrap()) as usize;
        let footer_bytes = file
            .read(vec![0; len - mem::size_of::<u64>() - footer_offset], footer_offset)
            .await;
        let footer = Rc::new(unsafe { from_bytes_unchecked::<Footer, Error>(&footer_bytes).unwrap() });

        self.inner.insert(id, footer.clone());
        footer
    }
}
