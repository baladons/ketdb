use std::ops::Index;

pub enum Batch<'a> {
    Fixed(&'a [u8], usize),
    Variable(&'a [u8], &'a [u32]),
}

impl Batch<'static> {
    pub fn empty() -> Self {
        Batch::Variable(&[], &[])
    }
}

impl<'a> Batch<'a> {
    pub fn len(&self) -> usize {
        match self {
            Batch::Fixed(data, width) => data.len() / *width,
            Batch::Variable(_, offsets) => offsets.len().saturating_sub(1),
        }
    }

    pub fn truncate(self, new_len: usize) -> Self {
        match self {
            Batch::Fixed(data, width) => {
                if width == 0 {
                    return Batch::Fixed(&[], 0);
                }
                let n = new_len.min(self.len());
                Batch::Fixed(&data[..n * width], width)
            }
            Batch::Variable(data, offsets) => {
                let max_rows = offsets.len().saturating_sub(1);
                let n = new_len.min(max_rows);
                if n == 0 {
                    return Batch::Variable(&[], &offsets[..1.min(offsets.len())]);
                }
                let end = offsets[n] as usize;
                Batch::Variable(&data[..end], &offsets[..n + 1])
            }
        }
    }
}

impl Index<usize> for Batch<'_> {
    type Output = [u8];

    fn index(&self, index: usize) -> &Self::Output {
        match self {
            Batch::Fixed(data, width) => {
                let start = index * *width;
                let end = start + *width;
                &data[start..end]
            }
            Batch::Variable(data, offsets) => {
                let start = offsets[index] as usize;
                let end = offsets[index + 1] as usize;
                &data[start..end]
            }
        }
    }
}
