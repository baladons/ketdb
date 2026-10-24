use crate::buffer::Buffer;

pub struct BufferPool {
    temp_buffers: Vec<Buffer>,
}

impl BufferPool {
    pub fn new() -> Self {
        Self {
            temp_buffers: Vec::new(),
        }
    }

    pub fn get(&mut self, size: usize) -> Buffer {
        let mut buffer = self
            .temp_buffers
            .pop()
            .unwrap_or_else(|| Buffer::with_capacity(size));

        buffer.clear();
        buffer
    }

    pub fn recycle(&mut self, buffer: Buffer) {
        self.temp_buffers.push(buffer);
    }
}
