use std::{
    mem::MaybeUninit,
    ops::{Deref, DerefMut},
};

use compio::buf::{IoBuf, IoBufMut, ReserveError, ReserveExactError, SetLen};
use rkyv::util::AlignedVec;

pub const PAGE_SIZE: usize = 4096;

pub struct Buffer {
    inner: AlignedVec<PAGE_SIZE>,
    read_capacity: usize,
}

impl Buffer {
    pub fn new() -> Self {
        Self {
            read_capacity: PAGE_SIZE,
            inner: AlignedVec::with_capacity(PAGE_SIZE),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let capacity = capacity.next_multiple_of(PAGE_SIZE);
        Self {
            read_capacity: capacity,
            inner: AlignedVec::with_capacity(capacity),
        }
    }

    pub fn inner(&self) -> &AlignedVec<PAGE_SIZE> {
        &self.inner
    }

    pub fn inner_mut(&mut self) -> &mut AlignedVec<PAGE_SIZE> {
        &mut self.inner
    }

    pub fn before_read(&mut self, size: usize) {
        self.read_capacity = size.next_multiple_of(PAGE_SIZE);
        self.inner.clear();
        self.inner.reserve(self.read_capacity);
    }

    pub fn after_read(&mut self, size: usize) {
        self.inner.resize(size, 0);
    }

    pub fn before_write(&mut self) {
        let next_len = self.inner.len().next_multiple_of(PAGE_SIZE);
        self.inner.resize(next_len, 0);
    }
}

impl IoBuf for Buffer {
    fn as_init(&self) -> &[u8] {
        self.inner.as_init()
    }
}

impl Deref for Buffer {
    type Target = AlignedVec<PAGE_SIZE>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for Buffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl IoBufMut for Buffer {
    fn as_uninit(&mut self) -> &mut [MaybeUninit<u8>] {
        unsafe {
            std::slice::from_raw_parts_mut(
                self.inner.as_mut_ptr().cast::<MaybeUninit<u8>>(),
                self.read_capacity,
            )
        }
    }

    fn reserve(&mut self, len: usize) -> Result<(), ReserveError> {
        self.inner.reserve(len);
        Ok(())
    }

    fn reserve_exact(&mut self, len: usize) -> Result<(), ReserveExactError> {
        self.inner.reserve_exact(len);
        Ok(())
    }
}

impl SetLen for Buffer {
    unsafe fn set_len(&mut self, len: usize) {
        unsafe { self.inner.set_len(len) };
    }
}

impl aes_gcm::aead::Buffer for Buffer {
    fn extend_from_slice(&mut self, other: &[u8]) -> aes_gcm::aead::Result<()> {
        self.inner.extend_from_slice(other);
        Ok(())
    }

    fn truncate(&mut self, len: usize) {
        self.inner.resize(len, 0);
    }
}

impl AsMut<[u8]> for Buffer {
    fn as_mut(&mut self) -> &mut [u8] {
        self.inner.as_mut()
    }
}

impl AsRef<[u8]> for Buffer {
    fn as_ref(&self) -> &[u8] {
        self.inner.as_ref()
    }
}
