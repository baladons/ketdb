#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum BlockType {
    // many per column
    Data,
    Min,
    Max,
    Bloom,
}

pub const KEY_COLUMN_ID: ColumnId = 0;
pub type ColumnId = u32;
pub type RowIndex = u32;
pub type BlockIndex = u32;
