use std::mem::MaybeUninit;

use compio::buf::{IoBuf, IoBufMut, ReserveError, ReserveExactError, SetLen};

pub const PAGE_SIZE: usize = 4096;

#[repr(C, align(4096))]
pub struct Page {
    bytes: [u8; PAGE_SIZE],
}

impl Page {
    fn zeroed() -> Self {
        Self {
            bytes: [0; PAGE_SIZE],
        }
    }
}

/// Byte buffer backed by 4096-byte pages for `O_DIRECT` I/O.
pub struct AlignedVec {
    pages: Vec<Page>,
    /// Bytes that are meaningful for the caller (not including page padding).
    logical_len: usize,
}

impl AlignedVec {
    pub fn new() -> Self {
        Self {
            pages: Vec::new(),
            logical_len: 0,
        }
    }

    pub fn from_slice(slice: &[u8]) -> Self {
        let mut buffer = Self::with_capacity_bytes(slice.len());
        buffer.extend_from_slice(slice);
        buffer
    }

    pub fn with_capacity_bytes(capacity: usize) -> Self {
        let page_count = capacity.div_ceil(PAGE_SIZE);
        Self {
            pages: Vec::with_capacity(page_count),
            logical_len: 0,
        }
    }

    pub fn with_io_capacity(io_len: usize) -> Self {
        assert_eq!(io_len % PAGE_SIZE, 0, "I/O length must be page-aligned");
        let page_count = io_len / PAGE_SIZE;
        let mut pages = Vec::with_capacity(page_count);
        for _ in 0..page_count {
            pages.push(Page::zeroed());
        }
        Self {
            pages,
            logical_len: 0,
        }
    }

    pub fn logical_len(&self) -> usize {
        self.logical_len
    }

    pub fn io_len(&self) -> usize {
        self.logical_len.div_ceil(PAGE_SIZE) * PAGE_SIZE
    }

    pub fn as_slice(&self) -> &[u8] {
        self.as_bytes(self.logical_len)
    }

    pub fn extend_from_slice(&mut self, data: &[u8]) {
        let start = self.logical_len;
        let end = start + data.len();
        self.ensure_pages_for(end);
        self.as_mut_bytes(end)[start..end].copy_from_slice(data);
        self.logical_len = end;
        self.zero_io_padding();
    }

    pub fn slice_range(&self, offset: usize, len: usize) -> &[u8] {
        assert!(offset + len <= self.logical_len);
        &self.as_slice()[offset..offset + len]
    }

    fn ensure_pages_for(&mut self, logical_len: usize) {
        let page_count = logical_len.div_ceil(PAGE_SIZE);
        while self.pages.len() < page_count {
            self.pages.push(Page::zeroed());
        }
    }

    fn zero_io_padding(&mut self) {
        let io_len = self.io_len();
        if self.logical_len < io_len {
            let start = self.logical_len;
            self.as_mut_bytes(io_len)[start..io_len].fill(0);
        }
    }

    fn as_bytes(&self, len: usize) -> &[u8] {
        if len == 0 {
            return &[];
        }
        assert!(len <= self.pages.len() * PAGE_SIZE);
        unsafe { std::slice::from_raw_parts(self.pages.as_ptr() as *const u8, len) }
    }

    fn as_mut_bytes(&mut self, len: usize) -> &mut [u8] {
        if len == 0 {
            return &mut [];
        }
        self.ensure_pages_for(len);
        unsafe { std::slice::from_raw_parts_mut(self.pages.as_mut_ptr() as *mut u8, len) }
    }
}

impl IoBuf for AlignedVec {
    fn as_init(&self) -> &[u8] {
        self.as_bytes(self.io_len())
    }
}

impl IoBufMut for AlignedVec {
    fn as_uninit(&mut self) -> &mut [MaybeUninit<u8>] {
        let cap = self.pages.len() * PAGE_SIZE;
        if cap == 0 {
            return &mut [];
        }
        unsafe {
            std::slice::from_raw_parts_mut(
                self.pages.as_mut_ptr() as *mut MaybeUninit<u8>,
                cap,
            )
        }
    }

    fn reserve(&mut self, len: usize) -> Result<(), ReserveError> {
        self.ensure_pages_for(self.logical_len + len);
        Ok(())
    }

    fn reserve_exact(&mut self, len: usize) -> Result<(), ReserveExactError> {
        self.reserve(len)?;
        Ok(())
    }
}

impl SetLen for AlignedVec {
    unsafe fn set_len(&mut self, len: usize) {
        self.ensure_pages_for(len);
        self.logical_len = len;
    }
}

pub fn page_align_down(offset: usize) -> usize {
    offset / PAGE_SIZE * PAGE_SIZE
}

pub fn page_align_up(offset: usize) -> usize {
    offset.div_ceil(PAGE_SIZE) * PAGE_SIZE
}
