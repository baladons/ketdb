use std::cell::UnsafeCell;

use crate::block_cache::BlockCache;
use crate::file_cache::FileCache;
use crate::mask_cache::MaskCache;
use crate::metadata::Metadata;
use crate::{footer_cache::FooterCache, key_vault::KeyVault};

thread_local! {
    pub static CONTEXT: UnsafeCell<Option<Context>> = UnsafeCell::new(None);
}

pub struct Context {
    pub file_cache: FileCache,
    pub block_cache: BlockCache,
    pub footer_cache: FooterCache,
    pub key_vault: KeyVault,
    pub mask_cache: MaskCache,
    pub metadata: Option<Metadata>,
}

impl Context {
    pub fn new(file_cache_capacity: usize) -> Self {
        Context {
            file_cache: FileCache::new(file_cache_capacity),
            block_cache: BlockCache::new(file_cache_capacity),
            footer_cache: FooterCache::new(file_cache_capacity),
            key_vault: KeyVault::new(),
            mask_cache: MaskCache::new(file_cache_capacity),
            metadata: None,
        }
    }

    pub fn clear_metadata() {
        CONTEXT.with(|cell| unsafe {
            if let Some(context) = (&mut *cell.get()).as_mut() {
                context.metadata = None;
            }
        });
    }

    pub fn into_current(self) {
        CONTEXT.with(|context| unsafe {
            *context.get() = Some(self);
        });
    }

    pub fn current() -> &'static mut Context {
        CONTEXT.with(|cell| unsafe { (&mut *cell.get()).as_mut().unwrap() })
    }
}
