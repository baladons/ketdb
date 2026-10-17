use rkyv::api::high::to_bytes_in;

use crate::{buffer::Buffer, dtos::BlockFooter, error::Error};

pub struct BlockMut {
    buffer: Buffer,
    footer: BlockFooter,
    min: usize,
    max: usize,
    len: usize,
}

impl BlockMut {
    pub fn new() -> Self {
        Self {
            buffer: Buffer::new(),
            footer: BlockFooter {
                offsets: Vec::new(),
                fixed_width: None,
            },
            min: 0,
            max: 0,
            len: 0,
        }
    }

    pub fn read(&self, index: usize) -> &[u8] {
        let start = self.footer.offsets[index] as usize;
        let end = self.footer.offsets[index + 1] as usize;
        &self.buffer[start..end]
    }

    pub fn write(&mut self, data: &[u8]) {
        self.update_fixed_width(data.len() as u32);

        let start = self.buffer.len();
        self.buffer.extend_from_slice(data);

        let index = self.footer.offsets.len();
        self.footer.offsets.push(start as u32);
        self.update_min_max(index);
        self.len += 1;
    }

    fn update_fixed_width(&mut self, value: u32) {
        if let Some(fixed_width) = self.footer.fixed_width {
            if fixed_width != value {
                self.footer.fixed_width = None;
            }
        } else if self.footer.offsets.is_empty() {
            self.footer.fixed_width = Some(value);
        }
    }

    fn update_min_max(&mut self, index: usize) {
        let current = self.read(index);

        let update_min = current < self.read(self.min);
        let update_max = current > self.read(self.max);

        if update_min {
            self.min = index;
        }
        if update_max {
            self.max = index;
        }
    }

    pub fn flush(&mut self) -> Result<(), Error> {
        let footer_offset = self.buffer.len() as u32;
        self.footer.offsets.push(footer_offset);

        to_bytes_in::<_, rkyv::rancor::Error>(&self.footer, &mut *self.buffer)
            .map_err(|_| Error::SerializationFailure)?;

        Ok(())
    }
}
