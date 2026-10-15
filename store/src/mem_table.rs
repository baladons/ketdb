use std::borrow::Borrow;
use std::cmp::Ordering;
use std::collections::{btree_map, BTreeMap};
use std::ops::Bound;

const INLINE_LEN: usize = 15;

/// Short values sit in the tree node. Longer values stay on the heap.
#[derive(Clone)]
pub enum Bytes {
    Inline { len: u8, data: [u8; INLINE_LEN] },
    Heap(Vec<u8>),
}

impl Bytes {
    pub fn from_slice(src: &[u8]) -> Self {
        if src.len() <= INLINE_LEN {
            let mut data = [0; INLINE_LEN];
            data[..src.len()].copy_from_slice(src);
            Self::Inline {
                len: src.len() as u8,
                data,
            }
        } else {
            Self::Heap(src.to_vec())
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        match self {
            Self::Inline { len, data } => &data[..*len as usize],
            Self::Heap(bytes) => bytes,
        }
    }
}

impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl Borrow<[u8]> for Bytes {
    fn borrow(&self) -> &[u8] {
        self.as_slice()
    }
}

impl PartialEq for Bytes {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl Eq for Bytes {}

impl PartialOrd for Bytes {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Bytes {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_slice().cmp(other.as_slice())
    }
}


use uuid::Uuid;

use crate::{
    delete_mask::{self, DeleteMask},
    metadata,
    table_cursor::TableCursor,
    table_reader::TableReader,
    table_writer::{ColumnConfig, TableWriter},
    types::{BlockType, ColumnId, TableGroupId},
    virtual_file::VirtualFile,
    wal::Wal,
};

pub const MEMTABLE_MAX_ENTRIES: usize = 100_000;

pub(crate) enum RowState<'a> {
    Live(&'a [Bytes]),
    Tombstone,
    Missing,
}

/// A position in the memtable B-tree. `peek` borrows the current entry and `advance` steps to the next key.
pub struct BtreeCursor<'a> {
    range: btree_map::Range<'a, Bytes, Option<Vec<Bytes>>>,
    current: Option<(&'a Bytes, &'a Option<Vec<Bytes>>)>,
}

impl<'a> BtreeCursor<'a> {
    fn new(mut range: btree_map::Range<'a, Bytes, Option<Vec<Bytes>>>) -> Self {
        let current = range.next();
        Self { range, current }
    }

    pub fn peek(&self) -> Option<(&'a Bytes, &'a Option<Vec<Bytes>>)> {
        self.current
    }

    pub fn advance(&mut self) {
        self.current = self.range.next();
    }
}

pub struct MemTable {
    group_id: TableGroupId,
    column_configs: Vec<ColumnConfig>,
    rows: BTreeMap<Bytes, Option<Vec<Bytes>>>,
    entries: usize,
    limit: usize,
    wal: Wal,
}

impl MemTable {
    pub async fn open(column_configs: Vec<ColumnConfig>) -> Self {
        Self::open_with_limit(column_configs, MEMTABLE_MAX_ENTRIES).await
    }

    pub async fn open_with_limit(column_configs: Vec<ColumnConfig>, limit: usize) -> Self {
        Self::open_in_group(0, column_configs, limit).await
    }

    pub async fn open_in_group(
        group_id: TableGroupId,
        column_configs: Vec<ColumnConfig>,
        limit: usize,
    ) -> Self {
        let column_ids = column_configs
            .iter()
            .map(|config| config.column_id)
            .collect();
        metadata::load().await.ensure_group(group_id, column_ids);
        let wal_bytes = metadata::load().await.group(group_id).wal_id;
        let wal_id = if wal_bytes == [0; 16] {
            let wal_id = Uuid::now_v7();
            metadata::load().await.group_mut(group_id).wal_id = *wal_id.as_bytes();
            metadata::store().await;
            wal_id
        } else {
            Uuid::from_bytes(wal_bytes)
        };

        let (wal, records) = Wal::open(wal_id).await;
        let entries = records.len();
        let mut rows = BTreeMap::new();
        for (key, value) in records {
            let value = value.map(|cells| cells.iter().map(|cell| Bytes::from_slice(cell)).collect());
            rows.insert(Bytes::from_slice(&key), value);
        }

        let mut memtable = Self {
            group_id,
            column_configs,
            rows,
            entries,
            limit,
            wal,
        };
        if memtable.entries >= memtable.limit {
            memtable.flush().await;
        }
        memtable
    }

    pub fn column_ids(&self) -> Vec<ColumnId> {
        self.column_configs
            .iter()
            .map(|config| config.column_id)
            .collect()
    }

    pub fn iter_entries(&self) -> impl Iterator<Item = (&Bytes, &Option<Vec<Bytes>>)> {
        self.rows.iter()
    }

    /// Entries from `start` through the greatest key. `None` walks the whole tree.
    pub fn entries_from(
        &self,
        start: Option<&[u8]>,
    ) -> btree_map::Range<'_, Bytes, Option<Vec<Bytes>>> {
        self.entry_range(start)
    }

    pub fn cursor(&self, start: Option<&[u8]>) -> BtreeCursor<'_> {
        BtreeCursor::new(self.entry_range(start))
    }

    fn entry_range(
        &self,
        start: Option<&[u8]>,
    ) -> btree_map::Range<'_, Bytes, Option<Vec<Bytes>>> {
        let lower = match start {
            Some(key) => Bound::Included(key),
            None => Bound::Unbounded,
        };
        self.rows
            .range::<[u8], (Bound<&[u8]>, Bound<&[u8]>)>((lower, Bound::Unbounded))
    }

    #[inline]
    pub fn get(&self, key: &[u8]) -> Option<&[Bytes]> {
        self.rows.get(key).and_then(|row| row.as_deref())
    }

    #[inline]
    pub(crate) fn row(&self, key: &[u8]) -> RowState<'_> {
        match self.rows.get(key) {
            Some(Some(cells)) => RowState::Live(cells),
            Some(None) => RowState::Tombstone,
            None => RowState::Missing,
        }
    }

    #[inline]
    pub(crate) fn column_position(&self, column_id: ColumnId) -> usize {
        self.column_configs
            .iter()
            .position(|config| config.column_id == column_id)
            .expect("missing column")
    }

    pub(crate) fn column_count(&self) -> usize {
        self.column_configs.len()
    }

    /// Replaces columns of a live memtable row. Other cells stay where they are.
    pub(crate) async fn patch_with(
        &mut self,
        key: &[u8],
        updates: &[(usize, &[u8])],
        buffered_journal: bool,
    ) -> bool {
        let record = {
            let Some(Some(cells)) = self.rows.get_mut(key) else {
                return false;
            };
            for &(index, value) in updates {
                cells[index] = Bytes::from_slice(value);
            }
            let refs: Vec<&[u8]> = cells.iter().map(Bytes::as_slice).collect();
            crate::wal::upsert_record(&refs)
        };
        self.wal.append_encoded(&record, buffered_journal).await;
        self.entries += 1;
        if self.entries >= self.limit {
            self.flush().await;
        }
        true
    }

    pub(crate) async fn insert_owned(&mut self, cells: Vec<Vec<u8>>, buffered_journal: bool) {
        let record = {
            let refs: Vec<&[u8]> = cells.iter().map(Vec::as_slice).collect();
            crate::wal::upsert_record(&refs)
        };
        self.wal.append_encoded(&record, buffered_journal).await;
        let key = Bytes::from_slice(&cells[0]);
        let stored: Vec<Bytes> = cells.iter().map(|cell| Bytes::from_slice(cell)).collect();
        self.rows.insert(key, Some(stored));
        self.entries += 1;
        if self.entries >= self.limit {
            self.flush().await;
        }
    }

    pub fn has_value(&self, column_id: ColumnId, value: &[u8]) -> bool {
        let Some(index) = self
            .column_configs
            .iter()
            .position(|config| config.column_id == column_id)
        else {
            return false;
        };
        self.rows.values().any(|row| {
            row.as_ref()
                .is_some_and(|cells| cells[index].as_slice() == value)
        })
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn entries(&self) -> usize {
        self.entries
    }

    pub fn wal_id(&self) -> Uuid {
        self.wal.id()
    }

    pub fn group_id(&self) -> TableGroupId {
        self.group_id
    }

    pub async fn flushed_tables(&self) -> Vec<Uuid> {
        metadata::load().await.group(self.group_id).tables.clone()
    }

    pub async fn insert(&mut self, values: &[&[u8]]) {
        self.insert_with(values, false).await;
    }

    pub(crate) async fn insert_with(&mut self, values: &[&[u8]], buffered_journal: bool) {
        assert_eq!(values.len(), self.column_configs.len());
        self.wal.append(values, buffered_journal).await;

        let key = Bytes::from_slice(values[0]);
        let cells: Vec<Bytes> = values.iter().map(|value| Bytes::from_slice(value)).collect();
        self.rows.insert(key, Some(cells));
        self.entries += 1;

        if self.entries >= self.limit {
            self.flush().await;
        }
    }

    pub async fn delete(&mut self, key: &[u8]) {
        self.delete_with(key, false).await;
    }

    pub(crate) async fn delete_with(&mut self, key: &[u8], buffered_journal: bool) {
        self.wal.append_delete(key, buffered_journal).await;
        self.rows.insert(Bytes::from_slice(key), None);
        self.entries += 1;

        if self.entries >= self.limit {
            self.flush().await;
        }
    }

    pub async fn flush(&mut self) {
        if self.rows.is_empty() {
            return;
        }

        let rows = std::mem::take(&mut self.rows);
        self.entries = 0;

        let tables = metadata::load().await.group(self.group_id).tables.clone();
        let mut masks = Vec::new();
        for id in &tables {
            let reader = TableReader::new(*id);
            let row_count = delete_mask::table_rows(&reader).await;
            masks.push((*id, DeleteMask::load(*id, row_count).await));
        }

        let mut deleted = 0u64;
        let mut live = Vec::new();
        for (key, value) in &rows {
            if let Some((table, row)) = locate_row(key.as_slice(), &tables, &masks).await {
                if let Some((_, mask)) = masks.iter_mut().find(|(id, _)| *id == table) {
                    if mask.set(row) {
                        deleted += 1;
                    }
                }
            }
            if let Some(cells) = value {
                live.push(cells.clone());
            }
        }

        let table_id = if live.is_empty() {
            None
        } else {
            Some(self.write_table(&live).await)
        };

        for (id, mask) in &masks {
            mask.store(*id).await;
        }

        let old_wal = self.wal.id();
        let new_wal_id = Uuid::now_v7();
        {
            let group = metadata::load().await.group_mut(self.group_id);
            group.deleted_rows += deleted;
            if let Some(table_id) = table_id {
                group.tables.insert(0, table_id);
            }
            group.wal_id = *new_wal_id.as_bytes();
        }
        metadata::store().await;

        self.wal = Wal::open(new_wal_id).await.0;
        VirtualFile::remove(old_wal);
        self.compact().await;
    }

    async fn compact(&self) {
        loop {
            let tables = metadata::load().await.group(self.group_id).tables.clone();
            let mut merged_index = None;
            for index in 0..tables.len().saturating_sub(1) {
                let live = live_row_count(tables[index]).await + live_row_count(tables[index + 1]).await;
                if live <= MEMTABLE_MAX_ENTRIES as u64 {
                    merged_index = Some(index);
                    break;
                }
            }
            let Some(index) = merged_index else {
                break;
            };
            self.merge_tables(index, tables[index], tables[index + 1]).await;
        }
    }

    async fn merge_tables(&self, index: usize, newer: Uuid, older: Uuid) {
        let newer_rows = self.read_live_rows(newer).await;
        let older_rows = self.read_live_rows(older).await;
        let rows = merge_live_rows(newer_rows, older_rows);
        let merged = if rows.is_empty() {
            None
        } else {
            Some(self.write_table(&rows).await)
        };

        let newer_deleted = deleted_in(newer).await;
        let older_deleted = deleted_in(older).await;
        {
            let group = metadata::load().await.group_mut(self.group_id);
            group.deleted_rows = group
                .deleted_rows
                .saturating_sub(newer_deleted + older_deleted);
            match merged {
                Some(id) => {
                    group.tables[index] = id;
                    group.tables.remove(index + 1);
                }
                None => {
                    group.tables.drain(index..index + 2);
                }
            }
        }
        metadata::store().await;

        VirtualFile::remove(newer);
        VirtualFile::remove(older);
    }

    async fn read_live_rows(&self, id: Uuid) -> Vec<Vec<Bytes>> {
        let reader = TableReader::new(id);
        let row_count = delete_mask::table_rows(&reader).await;
        if row_count == 0 {
            return Vec::new();
        }
        let mask = DeleteMask::load(id, row_count).await;
        let column_ids = self.column_ids();
        let mut cursor = TableCursor::seek_to_row_index(reader, 0, &column_ids).await;
        let mut rows = Vec::new();
        let mut index = 0u32;
        loop {
            let batches = cursor.next(1024).await;
            let len = batches.first().map(|batch| batch.len()).unwrap_or(0);
            if len == 0 {
                break;
            }
            for row in 0..len {
                if !mask.is_set(index) {
                    let cells = (0..column_ids.len())
                        .map(|cell| Bytes::from_slice(batches[cell].get(row)))
                        .collect();
                    rows.push(cells);
                }
                index += 1;
            }
        }
        rows
    }

    async fn write_table(&self, rows: &[Vec<Bytes>]) -> Uuid {
        let table_id = Uuid::now_v7();
        let mut configs = self.column_configs.clone();
        let expected = rows.len().max(1);
        for config in &mut configs {
            config.bloom_expected_items = expected;
        }

        let mut writer = TableWriter::new(table_id, configs);
        for cells in rows {
            for (config, value) in self.column_configs.iter().zip(cells.iter()) {
                writer
                    .write(BlockType::Data, config.column_id, value.as_slice())
                    .await;
            }
        }
        writer.finish().await;
        DeleteMask::new(rows.len() as u32).store(table_id).await;
        table_id
    }
}

async fn live_row_count(id: Uuid) -> u64 {
    let reader = TableReader::new(id);
    let rows = delete_mask::table_rows(&reader).await;
    let deleted = DeleteMask::load(id, rows).await.deleted_count(rows);
    u64::from(rows - deleted)
}

async fn deleted_in(id: Uuid) -> u64 {
    let reader = TableReader::new(id);
    let rows = delete_mask::table_rows(&reader).await;
    u64::from(DeleteMask::load(id, rows).await.deleted_count(rows))
}

fn merge_live_rows(newer: Vec<Vec<Bytes>>, older: Vec<Vec<Bytes>>) -> Vec<Vec<Bytes>> {
    let mut merged = Vec::new();
    let mut newer_index = 0;
    let mut older_index = 0;
    while newer_index < newer.len() || older_index < older.len() {
        if older_index >= older.len()
            || (newer_index < newer.len() && newer[newer_index][0] <= older[older_index][0])
        {
            if newer_index < newer.len()
                && older_index < older.len()
                && newer[newer_index][0] == older[older_index][0]
            {
                older_index += 1;
            }
            merged.push(newer[newer_index].clone());
            newer_index += 1;
        } else {
            merged.push(older[older_index].clone());
            older_index += 1;
        }
    }
    merged
}

async fn locate_row(
    key: &[u8],
    tables: &[Uuid],
    masks: &[(Uuid, DeleteMask)],
) -> Option<(Uuid, u32)> {
    for id in tables {
        let reader = TableReader::new(*id);
        let rows = delete_mask::table_rows(&reader).await;
        if rows == 0 {
            continue;
        }
        let row = reader.row_index_by_key(key).await;
        if row >= rows {
            continue;
        }
        if masks
            .iter()
            .find(|(table, _)| table == id)
            .is_some_and(|(_, mask)| mask.is_set(row))
        {
            continue;
        }
        if delete_mask::key_at(&reader, row).await.as_deref() == Some(key) {
            return Some((*id, row));
        }
    }
    None
}
