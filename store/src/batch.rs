pub struct Batch<'a> {
    pub data: &'a [u8],
    pub layout: BatchLayout<'a>,
}

pub enum BatchLayout<'a> {
    Fixed(usize),
    /// Every value is empty, so the byte length cannot recover the row count.
    Empty(usize),
    Variable(&'a [u32]),
}

impl Batch<'_> {
    pub fn len(&self) -> usize {
        match self.layout {
            BatchLayout::Fixed(width) => self.data.len() / width,
            BatchLayout::Empty(count) => count,
            BatchLayout::Variable(offsets) => offsets.len().saturating_sub(1),
        }
    }

    pub fn get(&self, index: usize) -> &[u8] {
        match self.layout {
            BatchLayout::Fixed(width) => {
                let start = index * width;
                &self.data[start..start + width]
            }
            BatchLayout::Empty(_) => &[],
            BatchLayout::Variable(offsets) => {
                let base = offsets[0] as usize;
                let start = offsets[index] as usize - base;
                let end = offsets[index + 1] as usize - base;
                &self.data[start..end]
            }
        }
    }

    pub fn truncate(&mut self, new_len: usize) {
        match self.layout {
            BatchLayout::Fixed(width) => {
                let new_data_len = new_len * width;
                self.data = &self.data[..new_data_len];
            }
            BatchLayout::Empty(_) => {
                self.layout = BatchLayout::Empty(new_len);
            }
            BatchLayout::Variable(offsets) => {
                if new_len == 0 || offsets.is_empty() {
                    self.data = &[];
                    self.layout = BatchLayout::Variable(&[]);
                    return;
                }

                let new_offsets = &offsets[..=new_len];
                let base = offsets[0] as usize;
                let new_data_len = new_offsets[new_len] as usize - base;
                self.data = &self.data[..new_data_len];
                self.layout = BatchLayout::Variable(new_offsets);
            }
        }
    }
}
