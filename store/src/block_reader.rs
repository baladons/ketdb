use std::rc::Rc;

use crate::{
    batch::{Batch, BatchLayout},
    binary_search::binary_search,
    types::RowIndex,
};

#[derive(Clone)]
pub struct BlockReader {
    inner: Rc<[u8]>,
}

impl From<Vec<u8>> for BlockReader {
    fn from(inner: Vec<u8>) -> Self {
        Self {
            inner: Rc::from(inner),
        }
    }
}

impl BlockReader {
    pub fn checksum(&self) -> u32 {
        crc32c::crc32c(&self.inner)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.inner
    }

    fn as_u32_slice(&self) -> &[u32] {
        bytemuck::cast_slice(&self.inner)
    }

    pub fn count(&self) -> u32 {
        let slice = self.as_u32_slice();
        slice[slice.len() - 1]
    }

    pub fn fixed_width(&self) -> Option<u32> {
        let slice = self.as_u32_slice();
        let fixed_width = slice[slice.len() - 2];
        if fixed_width != 0 {
            return Some(fixed_width);
        }

        // A zero width is the variable-width sentinel, unless the block has no offset
        // table. That is a fixed-width block whose values are all empty.
        let count = slice[slice.len() - 1] as usize;
        if slice.len() < count + 2 {
            Some(0)
        } else {
            None
        }
    }

    fn jump_offset(&self, internal_index: u32) -> usize {
        (self.as_u32_slice().len() as u32 - 3 - self.count() + internal_index) as usize
    }

    pub fn jump(&self, internal_index: u32) -> usize {
        let offset = match self.fixed_width() {
            Some(fixed_width) => internal_index * fixed_width,
            None => self.as_u32_slice()[self.jump_offset(internal_index)],
        };

        offset as usize
    }

    pub fn get(&self, internal_index: u32) -> &[u8] {
        let start = self.jump(internal_index);
        let end = self.jump(internal_index + 1);
        &self.inner[start..end]
    }

    pub async fn internal_index_by_key(&self, key: &[u8]) -> RowIndex {
        binary_search(self.count(), async |i| self.get(i).cmp(key)).await
    }

    pub fn batch(&self, internal_index: u32, batch_size: u32) -> Batch<'_> {
        let end_index = (internal_index + batch_size).min(self.count());
        let start_offset = self.jump(internal_index);
        let end_offset = self.jump(end_index);
        let data = &self.inner[start_offset..end_offset];

        let layout = match self.fixed_width() {
            Some(0) => BatchLayout::Empty((end_index - internal_index) as usize),
            Some(fixed_width) => BatchLayout::Fixed(fixed_width as usize),
            None => {
                let slice = self.as_u32_slice();
                let offsets =
                    &slice[self.jump_offset(internal_index)..=self.jump_offset(end_index)];
                BatchLayout::Variable(offsets)
            }
        };

        Batch { data, layout }
    }
}
