use std::{cmp::Ordering, convert::Infallible, rc::Rc};

use rkyv::{access, access_unchecked, option::ArchivedOption, rend::u32_le};

use crate::{
    batch::Batch,
    binary_search::{binary_search, SearchResult},
    buffer::Buffer,
    dtos::ArchivedBlockFooter,
    error::Error,
};

pub struct Block {
    buffer: Rc<Buffer>,
    footer_offset: usize,
}

impl Block {
    pub fn new(buffer: Rc<Buffer>, footer_offset: usize) -> Result<Self, Error> {
        access::<ArchivedBlockFooter, rkyv::rancor::Error>(&buffer[footer_offset..])
            .map_err(|_| Error::DeserializationFailure)?;

        Ok(Self {
            buffer,
            footer_offset,
        })
    }

    pub fn from_buffer(buffer: Rc<Buffer>) -> Result<Self, Error> {
        let footer_offset = find_footer_offset(&buffer)?;
        Self::new(buffer, footer_offset)
    }

    pub fn len(&self) -> usize {
        let footer = self.footer();
        match footer.fixed_width {
            ArchivedOption::None => footer.offsets.len().saturating_sub(1),
            ArchivedOption::Some(fixed_width) => {
                let fixed_width = fixed_width.to_native() as usize;
                if fixed_width == 0 {
                    0
                } else {
                    self.footer_offset / fixed_width
                }
            }
        }
    }

    pub async fn search(&self, key: &[u8]) -> SearchResult {
        binary_search(self.len(), async |mid| {
            Ok::<Ordering, Infallible>(self.get(mid).cmp(key))
        })
        .await
        .expect("infallible compare")
    }

    pub fn get(&self, index: usize) -> &[u8] {
        let footer = self.footer();
        match footer.fixed_width {
            ArchivedOption::Some(fixed_width) => {
                let fixed_width = fixed_width.to_native() as usize;
                let start = fixed_width * index;
                &self.buffer[start..start + fixed_width]
            }
            ArchivedOption::None => {
                let offsets = &footer.offsets.as_slice()[index..index + 2];
                let offsets = u32_le_slice_to_native(offsets);
                let start = offsets[0] as usize;
                let end = offsets[1] as usize;
                &self.buffer[start..end]
            }
        }
    }

    pub fn full_batch(&self) -> Batch<'_> {
        let footer = self.footer();
        match footer.fixed_width {
            ArchivedOption::Some(fixed_width) => {
                let fixed_width = fixed_width.to_native() as usize;
                Batch::Fixed(&self.buffer[..self.footer_offset], fixed_width)
            }
            ArchivedOption::None => self.batch(0),
        }
    }

    pub fn batch(&self, index: usize) -> Batch<'_> {
        let footer = self.footer();

        match footer.fixed_width {
            ArchivedOption::None => {
                let offsets = &footer.offsets.as_slice()[index..];
                let offsets = u32_le_slice_to_native(offsets);
                let start = offsets[0] as usize;
                let end = offsets.last().copied().expect("unreachable") as usize;
                let data = &self.buffer[start..end];
                Batch::Variable(data, offsets)
            }
            ArchivedOption::Some(fixed_width) => {
                let fixed_width = fixed_width.to_native() as usize;
                let start = fixed_width * index;
                let end = start + fixed_width;
                let data = &self.buffer[start..end];
                Batch::Fixed(data, fixed_width)
            }
        }
    }

    fn footer(&self) -> &ArchivedBlockFooter {
        // SAFETY: we already checked it in new
        unsafe { access_unchecked(&self.buffer[self.footer_offset..]) }
    }
}

fn find_footer_offset(buffer: &[u8]) -> Result<usize, Error> {
    for p in 0..buffer.len() {
        if let Ok(footer) = access::<ArchivedBlockFooter, rkyv::rancor::Error>(&buffer[p..]) {
            if footer
                .offsets
                .last()
                .is_some_and(|last| last.to_native() as usize == p)
            {
                return Ok(p);
            }
        }
    }

    Err(Error::DeserializationFailure)
}

fn u32_le_slice_to_native(slice: &[u32_le]) -> &[u32] {
    #[cfg(not(target_endian = "little"))]
    compile_error!("not supported");

    let ptr = slice.as_ptr() as *const u32;
    let len = slice.len();
    unsafe { std::slice::from_raw_parts(ptr, len) }
}
