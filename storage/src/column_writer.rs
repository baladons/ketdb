use crate::{
    block_writer::BlockWriter,
    dtos::{ColumnHandle, Encoding, Frame},
    error::Error,
    frame_writer::FrameWriter,
    storage::Storage,
};

pub struct ColumnWriter {
    id: u32,
    data: BlockWriter,
    min: BlockWriter,
    max: BlockWriter,
}

impl ColumnWriter {
    pub fn new(id: u32, block_flush_threshold: usize, encoding: Encoding) -> Self {
        Self {
            id,
            data: BlockWriter::new(block_flush_threshold, encoding),
            min: BlockWriter::new(block_flush_threshold, encoding),
            max: BlockWriter::new(block_flush_threshold, encoding),
        }
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub async fn write(
        &mut self,
        frame_writer: &mut FrameWriter,
        storage: &mut Storage,
        data: &[u8],
    ) -> Result<(), Error> {
        let will_flush = self.data.write(data, frame_writer, storage).await?;

        if will_flush {
            self.min
                .write(self.data.min_value(), frame_writer, storage)
                .await?;
            self.max
                .write(self.data.max_value(), frame_writer, storage)
                .await?;
        }

        Ok(())
    }

    pub async fn finish(
        mut self,
        frame_writer: &mut FrameWriter,
        storage: &mut Storage,
    ) -> Result<ColumnHandle, Error> {
        if !self.data.is_empty() {
            self.min
                .write(self.data.min_value(), frame_writer, storage)
                .await?;
            self.max
                .write(self.data.max_value(), frame_writer, storage)
                .await?;
        }

        Ok(ColumnHandle {
            id: self.id,
            data_blocks: self.data.finish(frame_writer, storage).await?,
            min_blocks: self.min.finish(frame_writer, storage).await?,
            max_blocks: self.max.finish(frame_writer, storage).await?,
            bloom_filter: Frame {
                encoding: Encoding::plain(),
                uncompressed_size: 0,
                crc: 0,
                len: 0,
                offset: 0,
            },
        })
    }
}
