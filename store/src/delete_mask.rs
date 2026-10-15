use uuid::Uuid;

use crate::{
    aligned_vec::PAGE_SIZE,
    context::Context,
    table_reader::TableReader,
    types::{BlockType, KEY_COLUMN_ID},
    virtual_file::VirtualFile,
};

/// One bit per row. Bit `row % 8` of byte `row / 8` is set when that row is deleted.
pub struct DeleteMask {
    bits: Vec<u8>,
}

impl DeleteMask {
    pub fn new(rows: u32) -> Self {
        Self {
            bits: vec![0; rows.div_ceil(8) as usize],
        }
    }

    pub async fn load(id: Uuid, rows: u32) -> Self {
        let mut mask = Self::new(rows);
        let logical = mask.bits.len();
        let mut offset = 0;
        while offset < logical {
            let page = Context::current()
                .mask_cache
                .get(id, (offset / PAGE_SIZE) as u32)
                .await;
            let n = (logical - offset).min(PAGE_SIZE);
            mask.bits[offset..offset + n].copy_from_slice(&page[..n]);
            offset += n;
        }
        mask
    }

    pub async fn store(&self, id: Uuid) {
        VirtualFile::mask(id).write_at(0, &self.bits).await;
        let mut offset = 0;
        while offset < self.bits.len() {
            let mut page = [0; PAGE_SIZE];
            let n = (self.bits.len() - offset).min(PAGE_SIZE);
            page[..n].copy_from_slice(&self.bits[offset..offset + n]);
            Context::current()
                .mask_cache
                .insert(id, (offset / PAGE_SIZE) as u32, page);
            offset += n;
        }
    }

    pub fn deleted_count(&self, rows: u32) -> u32 {
        (0..rows).filter(|row| self.is_set(*row)).count() as u32
    }

    pub fn is_set(&self, row: u32) -> bool {
        let index = (row / 8) as usize;
        if index >= self.bits.len() {
            return false;
        }
        self.bits[index] & (1 << (row % 8)) != 0
    }

    /// Returns whether this call newly marked the row.
    pub fn set(&mut self, row: u32) -> bool {
        let index = (row / 8) as usize;
        if index >= self.bits.len() {
            self.bits.resize(index + 1, 0);
        }
        let bit = 1 << (row % 8);
        let already = self.bits[index] & bit != 0;
        self.bits[index] |= bit;
        !already
    }
}

/// Tests one bit on the cached mask page.
pub async fn is_set(id: Uuid, row: u32) -> bool {
    let page_index = (row / 8) / PAGE_SIZE as u32;
    let byte = (row as usize / 8) % PAGE_SIZE;
    let bit = 1 << (row % 8);
    Context::current()
        .mask_cache
        .with_page(id, page_index, |page| page[byte] & bit != 0)
        .await
}

pub async fn table_rows(reader: &TableReader) -> u32 {
    let blocks = reader.len(BlockType::Data, KEY_COLUMN_ID).await;
    if blocks == 0 {
        return 0;
    }
    let last = blocks - 1;
    let start = reader
        .start_row_index(BlockType::Data, KEY_COLUMN_ID, last)
        .await;
    let block = reader.read(BlockType::Data, KEY_COLUMN_ID, last).await;
    start + block.count()
}

