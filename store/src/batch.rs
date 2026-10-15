pub struct Batch<'a> {
    pub data: &'a [u8],
    pub layout: BatchLayout<'a>,
}

pub enum BatchLayout<'a> {
    Fixed(usize),
    Variable(&'a [u32]),
}

impl Batch<'_> {
    pub fn len(&self) -> usize {
        match self.layout {
            BatchLayout::Fixed(width) => self.data.len() / width,
            BatchLayout::Variable(offsets) => offsets.len(),
        }
    }

    pub fn truncate(&mut self, new_len: usize) {
        match self.layout {
            BatchLayout::Fixed(width) => {
                let new_data_len = new_len * width;
                self.data = &self.data[..new_data_len];
            }
            BatchLayout::Variable(offsets) => {
                let new_offsets = &offsets[..new_len];
                let new_data_len = new_offsets.last().copied().unwrap_or(0) as usize;
                self.data = &self.data[..new_data_len];
                self.layout = BatchLayout::Variable(new_offsets);
            }
        }
    }
}
