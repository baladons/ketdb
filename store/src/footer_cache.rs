use std::{mem, rc::Rc};

use mini_moka::unsync::Cache;
use rkyv::{from_bytes_unchecked, rancor::Error};
use uuid::Uuid;

use crate::{
    aligned_vec::page_align_down, footer::Footer, virtual_file::VirtualFile,
};

const FOOTER_TRAILER_LEN: usize = mem::size_of::<u64>() * 2;

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
        let trailer_offset = page_align_down(len - 1);
        let trailer = file.read(FOOTER_TRAILER_LEN, trailer_offset).await;
        let footer_offset = u64::from_le_bytes(trailer[0..8].try_into().unwrap()) as usize;
        let footer_len = u64::from_le_bytes(trailer[8..16].try_into().unwrap()) as usize;
        let footer_bytes = file.read(footer_len, footer_offset).await;
        let footer = Rc::new(unsafe { from_bytes_unchecked::<Footer, Error>(&footer_bytes).unwrap() });

        self.inner.insert(id, footer.clone());
        footer
    }
}
