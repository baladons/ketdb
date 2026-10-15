use fastbloom::BloomFilter;
use rkyv::{rancor::Error, to_bytes};
use uuid::Uuid;

use crate::{
    block_reader::BlockReader,
    block_writer::BlockWriter,
    encoding::BlockEncoder,
    footer::{BlockEncoding, BlockHandle, Footer},
    types::{BlockType, ColumnId, RowIndex},
    virtual_file::VirtualFile,
};

pub struct ColumnConfig {
    pub column_id: ColumnId,
    pub block_size: usize,
    pub block_encoding: BlockEncoding,
}

pub struct TableWriter {
    id: Uuid,
    footer: Footer,
    encoder: BlockEncoder,
    data_writers: Vec<BlockWriter>,
    min_writers: Vec<BlockWriter>,
    max_writers: Vec<BlockWriter>,
    bloom_filters: Vec<Option<BloomFilter>>,
    column_configs: Vec<ColumnConfig>,
    row_indexes: Vec<RowIndex>,
    min_row_indexes: Vec<RowIndex>,
    max_row_indexes: Vec<RowIndex>,
}

impl TableWriter {
    pub fn new(id: Uuid, column_configs: Vec<ColumnConfig>) -> Self {
        let writers = || column_configs.iter().map(|_| BlockWriter::new()).collect();

        Self {
            id,
            footer: Footer::default(),
            encoder: BlockEncoder::new(),
            data_writers: writers(),
            min_writers: writers(),
            max_writers: writers(),
            bloom_filters: vec![None; column_configs.len()],
            row_indexes: vec![0; column_configs.len()],
            min_row_indexes: vec![0; column_configs.len()],
            max_row_indexes: vec![0; column_configs.len()],
            column_configs,
        }
    }

    pub async fn write(&mut self, block_type: BlockType, column_id: ColumnId, data: &[u8]) {
        let column_index = self
            .column_configs
            .iter()
            .position(|column| column.column_id == column_id)
            .expect("missing column config");

        let block_size = self.column_configs[column_index].block_size;
        let encoding = self.column_configs[column_index].block_encoding.clone();

        match block_type {
            BlockType::Data => {
                let data_writer = &mut self.data_writers[column_index];
                data_writer.write(data);

                if data_writer.len() >= block_size {
                    self.flush_data_block(column_index, column_id, encoding).await;
                }
            }
            BlockType::Min => {
                let writer = &mut self.min_writers[column_index];
                if writer.count() == 0 {
                    self.min_row_indexes[column_index] = self.row_indexes[column_index];
                }

                writer.write(data);
            }
            BlockType::Max => {
                let writer = &mut self.max_writers[column_index];
                if writer.count() == 0 {
                    self.max_row_indexes[column_index] = self.row_indexes[column_index];
                }

                writer.write(data);
            }
            BlockType::Bloom => panic!("cannot write bloom blocks"),
        }
    }

    pub async fn write_block(
        &mut self,
        block_type: BlockType,
        column_id: ColumnId,
        block: Vec<u8>,
        start_row_index: RowIndex,
        encoding: BlockEncoding,
    ) {
        let checksum = crc32c::crc32c(&block);
        let uncompressed_size = block.len() as u32;
        let encoded = self.encoder.encode(&block, &encoding).to_vec();

        let file = VirtualFile::new(self.id);
        let start_offset = file.len().await as u64;
        let end_offset = start_offset + encoded.len() as u64;
        file.append(&encoded).await;

        let handle = BlockHandle {
            start_row_index,
            start_offset,
            end_offset,
            checksum,
            encoding,
            uncompressed_size,
        };

        self.footer
            .block_handles_mut(column_id, block_type)
            .push(handle);
    }

    async fn flush_data_block(
        &mut self,
        column_index: usize,
        column_id: ColumnId,
        encoding: BlockEncoding,
    ) {
        let (block, start_row_index, flushed_rows, min, max) = {
            let data_writer = &mut self.data_writers[column_index];
            if data_writer.count() == 0 {
                return;
            }

            let start_row_index = self.row_indexes[column_index];
            let flushed_rows = data_writer.count();
            let min = data_writer.min().to_vec();
            let max = data_writer.max().to_vec();
            let block = data_writer.finish().to_vec();

            (block, start_row_index, flushed_rows, min, max)
        };

        let bloom_filter = self.bloom_filters[column_index].get_or_insert_with(|| {
            BloomFilter::with_false_pos(0.01).expected_items(flushed_rows as usize)
        });
        let block_reader = BlockReader::from(block.clone());
        for internal_index in 0..block_reader.count() {
            bloom_filter.insert(block_reader.get(internal_index));
        }

        self.write_block(
            BlockType::Data,
            column_id,
            block,
            start_row_index,
            encoding.clone(),
        )
        .await;

        self.row_indexes[column_index] = start_row_index + flushed_rows;

        {
            let min_writer = &mut self.min_writers[column_index];
            if min_writer.count() == 0 {
                self.min_row_indexes[column_index] = start_row_index;
            }

            min_writer.write(&min);
        }

        {
            let max_writer = &mut self.max_writers[column_index];
            if max_writer.count() == 0 {
                self.max_row_indexes[column_index] = start_row_index;
            }

            max_writer.write(&max);
        }
    }

    pub async fn finish(&mut self) {
        for column_index in 0..self.column_configs.len() {
            let column_id = self.column_configs[column_index].column_id;
            let encoding = self.column_configs[column_index].block_encoding.clone();

            if self.data_writers[column_index].count() != 0 {
                self.flush_data_block(column_index, column_id, encoding.clone())
                    .await;
            }

            if self.min_writers[column_index].count() != 0 {
                let start_row_index = self.min_row_indexes[column_index];
                let flushed_rows = self.min_writers[column_index].count();
                let block = self.min_writers[column_index].finish().to_vec();
                self.write_block(
                    BlockType::Min,
                    column_id,
                    block,
                    start_row_index,
                    encoding.clone(),
                )
                .await;
                self.min_row_indexes[column_index] = start_row_index + flushed_rows;
            }

            if self.max_writers[column_index].count() != 0 {
                let start_row_index = self.max_row_indexes[column_index];
                let flushed_rows = self.max_writers[column_index].count();
                let block = self.max_writers[column_index].finish().to_vec();
                self.write_block(
                    BlockType::Max,
                    column_id,
                    block,
                    start_row_index,
                    encoding.clone(),
                )
                .await;
                self.max_row_indexes[column_index] = start_row_index + flushed_rows;
            }

            if let Some(bloom_filter) = self.bloom_filters[column_index].take() {
                let mut bytes = Vec::with_capacity(4 + bloom_filter.as_slice().len() * 8);
                bytes.extend_from_slice(&bloom_filter.num_hashes().to_le_bytes());
                for word in bloom_filter.iter() {
                    bytes.extend_from_slice(&word.to_le_bytes());
                }

                if !bytes.is_empty() {
                    self.write_block(BlockType::Bloom, column_id, bytes, 0, encoding)
                        .await;
                }
            }
        }

        let file = VirtualFile::new(self.id);
        let footer_offset = file.len().await as u64;
        let footer_bytes = to_bytes::<Error>(&self.footer).unwrap().to_vec();
        file.append(&footer_bytes).await;
        file.append(&footer_offset.to_le_bytes()).await;
    }
}
