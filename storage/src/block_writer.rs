use crate::{
    block_mut::BlockMut,
    dtos::{BlockHandle, Encoding},
    error::Error,
    frame_writer::FrameWriter,
    storage::Storage,
};

pub struct BlockWriter {
    active: BlockMut,
    written: Vec<BlockHandle>,
    threshold: usize,
    encoding: Encoding,
    row_offset: u32,
}

impl BlockWriter {
    pub fn new(threshold: usize, encoding: Encoding) -> Self {
        Self {
            active: BlockMut::new(),
            written: Vec::new(),
            threshold,
            encoding,
            row_offset: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.active.is_empty()
    }

    pub fn min_value(&self) -> &[u8] {
        self.active.read(self.active.min_index())
    }

    pub fn max_value(&self) -> &[u8] {
        self.active.read(self.active.max_index())
    }

    fn over_threshold(&self) -> bool {
        self.active.buffer_len() > self.threshold
    }

    pub async fn finish(
        self,
        frame_writer: &mut FrameWriter,
        storage: &mut Storage,
    ) -> Result<Vec<BlockHandle>, Error> {
        let mut writer = self;
        writer.flush(frame_writer, storage).await?;
        Ok(writer.written)
    }

    pub async fn write(
        &mut self,
        data: &[u8],
        frame_writer: &mut FrameWriter,
        storage: &mut Storage,
    ) -> Result<bool, Error> {
        if self.over_threshold() {
            self.flush(frame_writer, storage).await?;
        }

        self.active.write(data);

        Ok(self.over_threshold())
    }

    async fn flush(
        &mut self,
        frame_writer: &mut FrameWriter,
        storage: &mut Storage,
    ) -> Result<(), Error> {
        if self.active.is_empty() {
            return Ok(());
        }

        let row_len = self.active.entry_count() as u32;
        let row_offset = self.row_offset;
        let mut block = std::mem::replace(&mut self.active, BlockMut::new());
        let footer_offset = block.flush()?;
        let buffer = block.into_buffer();
        let frame = frame_writer.write(storage, self.encoding, buffer).await?;

        self.written.push(BlockHandle {
            frame,
            footer_offset: footer_offset as u32,
            row_offset,
            row_len,
        });

        self.row_offset += row_len;

        Ok(())
    }
}
