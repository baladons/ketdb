use crate::{
    dtos::{ColumnHandle, Encoding, Frame},
    error::Error,
    context::Context,
};

use super::{block_writer::BlockWriter, frame_writer::FrameWriter};

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
        context: &mut Context,
        data: &[u8],
    ) -> Result<(), Error> {
        let will_flush = self.data.write(data, frame_writer, context).await?;

        if will_flush {
            self.write_min_max(frame_writer, context).await?;
        }

        Ok(())
    }

    async fn write_min_max(
        &mut self,
        frame_writer: &mut FrameWriter,
        context: &mut Context,
    ) -> Result<(), Error> {
        self.min
            .write(self.data.min_value(), frame_writer, context)
            .await?;
        self.max
            .write(self.data.max_value(), frame_writer, context)
            .await?;

        Ok(())
    }

    pub async fn finish(
        mut self,
        frame_writer: &mut FrameWriter,
        context: &mut Context,
    ) -> Result<ColumnHandle, Error> {
        if !self.data.is_empty() {
            self.write_min_max(frame_writer, context).await?;
        }

        Ok(ColumnHandle {
            id: self.id,
            data_blocks: self.data.finish(frame_writer, context).await?,
            min_blocks: self.min.finish(frame_writer, context).await?,
            max_blocks: self.max.finish(frame_writer, context).await?,
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
