use std::ops;

pub struct BlockWriter {
    buffer: Vec<u8>,
    finished: Vec<u8>,
    offsets: Vec<u32>,
    fixed_width: Option<usize>,
    is_first_write: bool,
    min: Option<ops::Range<usize>>,
    max: Option<ops::Range<usize>>,
}

impl BlockWriter {
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            finished: Vec::new(),
            offsets: Vec::new(),
            fixed_width: None,
            is_first_write: true,
            min: None,
            max: None,
        }
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn count(&self) -> u32 {
        self.offsets.len() as u32
    }

    fn update_fixed_width(&mut self, width: usize) {
        if self.is_first_write {
            self.fixed_width = Some(width);
        }

        self.is_first_write = false;

        if let Some(fixed_width) = self.fixed_width {
            if fixed_width != width {
                self.fixed_width = None;
            }
        }
    }

    fn update_min_max(&mut self, range: ops::Range<usize>) {
        let current = &self.buffer[range.clone()];
        let min = self.min.get_or_insert(range.clone());
        let max = self.max.get_or_insert(range.clone());

        if current < &self.buffer[min.clone()] {
            *min = range.clone();
        }

        if current > &self.buffer[max.clone()] {
            *max = range;
        }
    }

    fn pad_multiple_of_4(&mut self) {
        let padding = (4 - (self.buffer.len() % 4)) % 4;
        self.buffer.extend(std::iter::repeat(0).take(padding));
    }

    pub fn write(&mut self, data: &[u8]) {
        self.update_fixed_width(data.len());
        let start = self.buffer.len();
        self.buffer.extend_from_slice(data);
        let end = self.buffer.len();
        self.update_min_max(start..end);
        self.offsets.push(end as u32);
    }

    pub fn min(&self) -> &[u8] {
        &self.buffer[self.min.clone().unwrap()]
    }

    pub fn max(&self) -> &[u8] {
        &self.buffer[self.max.clone().unwrap()]
    }

    pub fn finish(&mut self) -> &[u8] {
        self.pad_multiple_of_4();

        if self.fixed_width.is_none() {
            self.buffer
                .extend_from_slice(bytemuck::cast_slice(&self.offsets));
        }

        let fixed_width = self.fixed_width.unwrap_or(0) as u32;
        let count = self.offsets.len() as u32;

        self.buffer.extend_from_slice(&fixed_width.to_le_bytes());
        self.buffer.extend_from_slice(&count.to_le_bytes());

        self.finished = std::mem::take(&mut self.buffer);
        self.offsets.clear();
        self.fixed_width = None;
        self.is_first_write = true;
        self.min = None;
        self.max = None;

        &self.finished
    }
}
