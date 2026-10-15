use std::collections::HashMap;
use std::fmt;
use std::future::{Future, ready};

use futures::future::Either;

use crate::{
    block_reader::BlockReader,
    delete_mask,
    footer::BlockEncoding,
    mem_table::{Bytes, MemTable, RowState, MEMTABLE_MAX_ENTRIES},
    merged_table_cursor::MergedTableCursor,
    metadata,
    table_reader::TableReader,
    table_writer::ColumnConfig,
    types::{BlockType, ColumnId, TableGroupId, KEY_COLUMN_ID},
    virtual_file::VirtualFile,
};

const INLINE_COLUMNS: usize = 8;

/// Cells from one point read. `get` borrows a memtable value or a cached block.
pub struct ReadRow<'a> {
    inner: ReadInner<'a>,
}

enum ReadInner<'a> {
    Mem {
        row: &'a [Bytes],
        columns: [u8; INLINE_COLUMNS],
        len: u8,
    },
    Blocks(Box<Vec<BlockCell>>),
}

struct BlockCell {
    reader: BlockReader,
    index: u32,
}

impl<'a> ReadRow<'a> {
    #[inline]
    fn from_mem(table: &'a MemTable, row: &'a [Bytes], column_ids: &[ColumnId]) -> Self {
        let mut columns = [0; INLINE_COLUMNS];
        let len = column_ids.len();
        assert!(len <= INLINE_COLUMNS, "too many columns in one read");
        for (slot, column_id) in column_ids.iter().enumerate() {
            columns[slot] = table.column_position(*column_id) as u8;
        }
        Self {
            inner: ReadInner::Mem {
                row,
                columns,
                len: len as u8,
            },
        }
    }

    fn from_blocks(columns: Vec<BlockCell>) -> Self {
        let len = columns.len();
        assert!(len <= INLINE_COLUMNS, "too many columns in one read");
        Self {
            inner: ReadInner::Blocks(Box::new(columns)),
        }
    }

    pub fn len(&self) -> usize {
        match &self.inner {
            ReadInner::Mem { len, .. } => *len as usize,
            ReadInner::Blocks(columns) => columns.len(),
        }
    }

    #[inline]
    pub fn get(&self, index: usize) -> &[u8] {
        match &self.inner {
            ReadInner::Mem { row, columns, .. } => row[columns[index] as usize].as_slice(),
            ReadInner::Blocks(columns) => columns[index].reader.get(columns[index].index),
        }
    }

    pub fn to_vec(&self) -> Vec<Vec<u8>> {
        (0..self.len()).map(|index| self.get(index).to_vec()).collect()
    }
}

impl PartialEq<Vec<Vec<u8>>> for ReadRow<'_> {
    fn eq(&self, other: &Vec<Vec<u8>>) -> bool {
        self.len() == other.len() && (0..self.len()).all(|index| self.get(index) == other[index])
    }
}

impl fmt::Debug for ReadRow<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries((0..self.len()).map(|index| self.get(index)))
            .finish()
    }
}

/// Controls when a write reaches the WAL file.
#[derive(Clone, Copy, Debug)]
pub struct WriteConcern {
    /// Keep journal records in memory until the 4096-byte page is full.
    /// When false, every write flushes the page.
    pub buffered_journal: bool,
}

/// One database. Each table group has its own memtable, WAL, and flushed tables.
pub struct Store {
    tables: HashMap<TableGroupId, MemTable>,
}

impl Store {
    pub async fn open() -> Self {
        let groups: Vec<_> = metadata::load()
            .await
            .groups
            .iter()
            .map(|group| (group.id, group.column_ids.clone()))
            .collect();
        let mut tables = HashMap::new();
        for (id, column_ids) in groups {
            let memtable =
                MemTable::open_in_group(id, configs_for(&column_ids), MEMTABLE_MAX_ENTRIES).await;
            tables.insert(id, memtable);
        }
        Self { tables }
    }

    pub async fn create_table(&mut self, id: TableGroupId, column_ids: &[ColumnId]) {
        assert!(!self.tables.contains_key(&id), "table already exists");
        let column_ids = schema(column_ids);
        let memtable =
            MemTable::open_in_group(id, configs_for(&column_ids), MEMTABLE_MAX_ENTRIES).await;
        self.tables.insert(id, memtable);
    }

    pub async fn delete_table(&mut self, id: TableGroupId) {
        let memtable = self.tables.remove(&id).expect("missing table");
        let group = metadata::load().await.remove_group(id);
        metadata::store().await;
        for table in group.tables {
            VirtualFile::remove(table);
        }
        VirtualFile::remove(memtable.wal_id());
    }

    /// Writes the memtable to a table file and clears it.
    pub async fn flush(&mut self, id: TableGroupId) {
        self.tables.get_mut(&id).expect("missing table").flush().await;
    }

    /// Writes `values` into the columns named by `column_ids`. Other columns on an existing row stay.
    pub async fn insert(
        &mut self,
        id: TableGroupId,
        column_ids: &[ColumnId],
        values: &[&[u8]],
        concern: WriteConcern,
    ) {
        self.write(id, column_ids, values, true, concern).await;
    }

    /// Patches an existing row. A missing row is left unchanged.
    pub async fn update(
        &mut self,
        id: TableGroupId,
        column_ids: &[ColumnId],
        values: &[&[u8]],
        concern: WriteConcern,
    ) {
        self.write(id, column_ids, values, false, concern).await;
    }

    /// Clears `column_ids` on the row identified by `key`. Including the key column deletes the row.
    pub async fn delete(
        &mut self,
        id: TableGroupId,
        column_ids: &[ColumnId],
        key: &[u8],
        concern: WriteConcern,
    ) {
        if column_ids.is_empty() {
            return;
        }
        if column_ids.contains(&KEY_COLUMN_ID) {
            self.tables
                .get_mut(&id)
                .expect("missing table")
                .delete_with(key, concern.buffered_journal)
                .await;
            return;
        }
        let table = self.tables.get_mut(&id).expect("missing table");
        let updates: Vec<(usize, &[u8])> = column_ids
            .iter()
            .map(|column_id| (table.column_position(*column_id), &b""[..]))
            .collect();
        apply_row(table, key, &updates, false, concern).await;
    }

    /// Values of `column_ids` for `key`, in the same order as `column_ids`.
    /// Each cell borrows the memtable or a cached data block.
    ///
    /// A memtable hit returns a ready future. The flushed-table search is boxed so a hit
    /// does not build that state machine.
    #[inline]
    pub fn read<'a>(
        &'a self,
        id: TableGroupId,
        column_ids: &'a [ColumnId],
        key: &'a [u8],
    ) -> impl Future<Output = Option<ReadRow<'a>>> + 'a {
        let table = self.tables.get(&id).expect("missing table");
        match table.row(key) {
            RowState::Tombstone => Either::Left(ready(None)),
            RowState::Live(cells) => {
                Either::Left(ready(Some(ReadRow::from_mem(table, cells, column_ids))))
            }
            RowState::Missing => Either::Right(Box::pin(read_flushed(table, column_ids, key))),
        }
    }

    async fn write(
        &mut self,
        id: TableGroupId,
        column_ids: &[ColumnId],
        values: &[&[u8]],
        insert: bool,
        concern: WriteConcern,
    ) {
        assert_eq!(column_ids.len(), values.len());
        let table = self.tables.get_mut(&id).expect("missing table");
        let key = values[column_index(column_ids, KEY_COLUMN_ID)];
        let updates: Vec<(usize, &[u8])> = column_ids
            .iter()
            .zip(values)
            .map(|(column_id, value)| (table.column_position(*column_id), *value))
            .collect();
        apply_row(table, key, &updates, insert, concern).await;
    }
}

fn schema(column_ids: &[ColumnId]) -> Vec<ColumnId> {
    let mut ids = vec![KEY_COLUMN_ID];
    for id in column_ids {
        if *id != KEY_COLUMN_ID && !ids.contains(id) {
            ids.push(*id);
        }
    }
    ids
}

fn configs_for(column_ids: &[ColumnId]) -> Vec<ColumnConfig> {
    let encoding = BlockEncoding {
        key_id: 0,
        compression: 0,
    };
    column_ids
        .iter()
        .map(|column_id| ColumnConfig {
            column_id: *column_id,
            block_size: 4096,
            block_encoding: encoding.clone(),
            bloom_expected_items: MEMTABLE_MAX_ENTRIES,
        })
        .collect()
}

fn column_index(column_ids: &[ColumnId], column_id: ColumnId) -> usize {
    column_ids
        .iter()
        .position(|id| *id == column_id)
        .expect("missing column")
}

async fn apply_row(
    table: &mut MemTable,
    key: &[u8],
    updates: &[(usize, &[u8])],
    insert: bool,
    concern: WriteConcern,
) {
    if table
        .patch_with(key, updates, concern.buffered_journal)
        .await
    {
        return;
    }
    if matches!(table.row(key), RowState::Tombstone) {
        if !insert {
            return;
        }
    } else if let Some(mut cells) = read_flushed_owned(table, key).await {
        for &(index, value) in updates {
            cells[index] = value.to_vec();
        }
        table.insert_owned(cells, concern.buffered_journal).await;
        return;
    } else if !insert {
        return;
    }

    let mut cells = vec![Vec::new(); table.column_count()];
    for &(index, value) in updates {
        cells[index] = value.to_vec();
    }
    table.insert_owned(cells, concern.buffered_journal).await;
}

async fn read_flushed_owned(table: &MemTable, key: &[u8]) -> Option<Vec<Vec<u8>>> {
    if metadata::load()
        .await
        .group(table.group_id())
        .tables
        .is_empty()
    {
        return None;
    }
    let mut cursor = MergedTableCursor::seek_memtable(table, key).await;
    let batch = cursor.next(1).await;
    if batch.len() == 0 || batch.get(0, 0) != key {
        return None;
    }
    Some(
        (0..batch.cells_per_row())
            .map(|cell| batch.get(0, cell).to_vec())
            .collect(),
    )
}

async fn read_flushed<'a>(
    table: &MemTable,
    column_ids: &[ColumnId],
    key: &[u8],
) -> Option<ReadRow<'a>> {
    let table_count = metadata::load().await.group(table.group_id()).tables.len();
    for index in 0..table_count {
        let table_id = metadata::load().await.group(table.group_id()).tables[index];
        let reader = TableReader::new(table_id);
        let rows = delete_mask::table_rows(&reader).await;
        if rows == 0 {
            continue;
        }
        let row = reader.row_index_by_key(key).await;
        if row >= rows {
            continue;
        }
        if delete_mask::is_set(table_id, row).await {
            continue;
        }
        if delete_mask::key_at(&reader, row).await.as_deref() != Some(key) {
            continue;
        }

        let mut columns = Vec::with_capacity(column_ids.len());
        for column_id in column_ids {
            let block_index = reader
                .data_block_index_by_row_index(*column_id, row)
                .await;
            let start = reader
                .start_row_index(BlockType::Data, *column_id, block_index)
                .await;
            let block = reader
                .read(BlockType::Data, *column_id, block_index)
                .await;
            columns.push(BlockCell {
                reader: block,
                index: row - start,
            });
        }
        return Some(ReadRow::from_blocks(columns));
    }
    None
}

