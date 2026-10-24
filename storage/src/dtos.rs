use rkyv::{Archive, Deserialize, Serialize};

#[derive(Clone, Archive, Deserialize, Serialize)]
pub struct SegmentFooter {
    pub columns: Vec<ColumnHandle>,
    pub wrapped_key: Option<Vec<u8>>,
}

#[derive(Clone, Archive, Deserialize, Serialize)]
pub struct ColumnHandle {
    pub id: u32,
    pub data_blocks: Vec<BlockHandle>,
    pub min_blocks: Vec<BlockHandle>,
    pub max_blocks: Vec<BlockHandle>,
    pub bloom_filter: Frame,
}

#[derive(Clone, Archive, Deserialize, Serialize)]
pub struct BlockHandle {
    pub frame: Frame,
    pub footer_offset: u32,
    pub row_offset: u32,
    pub row_len: u32,
}

#[derive(Debug, Clone, Copy, Archive, Deserialize, Serialize)]
pub struct Encoding {
    pub encrypted: bool,
    pub compression: u8, // 0 is no ZSTD compression
}

impl Encoding {
    pub fn plain() -> Self {
        Self {
            encrypted: false,
            compression: 0,
        }
    }

    pub fn compressed() -> Self {
        Self {
            encrypted: false,
            compression: 3,
        }
    }
}

#[derive(Debug, Clone, Archive, Deserialize, Serialize)]
pub struct Frame {
    pub encoding: Encoding,
    pub uncompressed_size: u32,
    pub crc: u32,
    pub len: u32,
    pub offset: u64,
}

#[derive(Clone, Archive, Deserialize, Serialize)]
pub struct BlockFooter {
    pub offsets: Vec<u32>,
    pub fixed_width: Option<u32>,
}
