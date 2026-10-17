use std::rc::Rc;

use rkyv::{access, access_unchecked, option::ArchivedOption, rend::u32_le};

use crate::{batch::Batch, buffer::Buffer, dtos::ArchivedBlockFooter, error::Error};

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
                let end = self.footer_offset;
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

fn u32_le_slice_to_native(slice: &[u32_le]) -> &[u32] {
    #[cfg(not(target_endian = "little"))]
    compile_error!("not supported");

    let ptr = slice.as_ptr() as *const u32;
    let len = slice.len();
    unsafe { std::slice::from_raw_parts(ptr, len) }
}
