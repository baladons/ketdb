use rkyv::api::high::to_bytes_in;

use crate::{
    buffer::Buffer,
    dtos::{Encoding, SegmentFooter},
    error::Error,
    file_id::FileId,
    context::Context,
};

use super::{column_writer::ColumnWriter, frame_writer::FrameWriter};

pub struct SegmentWriter {
    file_id: FileId,
    encoding: Encoding,
    block_threshold: usize,
    columns: Vec<ColumnWriter>,
}

impl SegmentWriter {
    pub fn new(file_id: FileId, block_threshold: usize) -> Self {
        Self {
            file_id,
            encoding: Encoding::plain(),
            block_threshold,
            columns: Vec::new(),
        }
    }

    pub async fn write(
        &mut self,
        frame_writer: &mut FrameWriter,
        context: &mut Context,
        column_id: u32,
        data: &[u8],
    ) -> Result<(), Error> {
        self.column_writer(column_id)
            .write(frame_writer, context, data)
            .await?;

        Ok(())
    }

    pub async fn finish(
        self,
        frame_writer: &mut FrameWriter,
        context: &mut Context,
    ) -> Result<FileId, Error> {
        let mut column_handles = Vec::with_capacity(self.columns.len());
        for column in self.columns {
            column_handles.push(column.finish(frame_writer, context).await?);
        }

        let footer = SegmentFooter {
            columns: column_handles,
            wrapped_key: None,
        };

        let mut buffer = Buffer::new();

        to_bytes_in::<_, rkyv::rancor::Error>(&footer, &mut *buffer)
            .map_err(|_| Error::SerializationFailure)?;

        frame_writer.finish(context, buffer).await?;

        Ok(self.file_id)
    }

    fn column_writer(&mut self, column_id: u32) -> &mut ColumnWriter {
        if let Some(index) = self.columns.iter().position(|c| c.id() == column_id) {
            return &mut self.columns[index];
        }

        self.columns.push(ColumnWriter::new(
            column_id,
            self.block_threshold,
            self.encoding,
        ));

        self.columns.last_mut().unwrap()
    }
}
