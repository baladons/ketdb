use std::rc::Rc;

use mini_moka::unsync::Cache;
use uuid::Uuid;

use crate::{aligned_vec::PAGE_SIZE, virtual_file::VirtualFile};

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
struct MaskPageKey {
    table_id: Uuid,
    page_index: u32,
}

/// Cached 4096-byte pages of `data/{table_id}.mask.ket`.
pub struct MaskCache {
    inner: Cache<MaskPageKey, Rc<[u8; PAGE_SIZE]>>,
}

impl MaskCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Cache::new(capacity as u64).into(),
        }
    }

    pub async fn get(&mut self, table_id: Uuid, page_index: u32) -> Rc<[u8; PAGE_SIZE]> {
        let key = MaskPageKey {
            table_id,
            page_index,
        };
        if let Some(page) = self.inner.get(&key) {
            return page.clone();
        }

        let page = read_page(table_id, page_index).await;
        self.inner.insert(key, page.clone());
        page
    }

    /// Runs `read` against the cached page. The page is loaded on a miss.
    pub async fn with_page<T>(
        &mut self,
        table_id: Uuid,
        page_index: u32,
        read: impl FnOnce(&[u8; PAGE_SIZE]) -> T,
    ) -> T {
        let key = MaskPageKey {
            table_id,
            page_index,
        };
        if self.inner.get(&key).is_none() {
            let page = read_page(table_id, page_index).await;
            self.inner.insert(key, page);
        }
        let page = self.inner.get(&key).expect("mask page");
        read(page)
    }

    pub fn insert(&mut self, table_id: Uuid, page_index: u32, page: [u8; PAGE_SIZE]) {
        self.inner.insert(
            MaskPageKey {
                table_id,
                page_index,
            },
            Rc::new(page),
        );
    }
}

async fn read_page(table_id: Uuid, page_index: u32) -> Rc<[u8; PAGE_SIZE]> {
    let file = VirtualFile::mask(table_id);
    let offset = page_index as usize * PAGE_SIZE;
    let len = file.len().await;
    if offset >= len {
        return Rc::new([0; PAGE_SIZE]);
    }

    let bytes = file.read(PAGE_SIZE, offset).await;
    let mut page = [0; PAGE_SIZE];
    let copy = bytes.len().min(PAGE_SIZE);
    page[..copy].copy_from_slice(&bytes[..copy]);
    Rc::new(page)
}
