use crate::{
    batch::Batch,
    delete_mask::DeleteMask,
    mem_table::{BtreeCursor, MemTable},
    table_cursor::TableCursor,
    table_reader::TableReader,
    types::{BlockType, ColumnId, RowIndex},
};

struct PendingRows {
    data: Vec<u8>,
    ends: Vec<u32>,
    cells_per_row: usize,
    pos: usize,
    deleted: Vec<bool>,
}

impl PendingRows {
    fn new(cells_per_row: usize) -> Self {
        Self {
            data: Vec::new(),
            ends: Vec::new(),
            cells_per_row,
            pos: 0,
            deleted: Vec::new(),
        }
    }

    fn remaining(&self) -> usize {
        if self.cells_per_row == 0 {
            0
        } else {
            self.ends.len() / self.cells_per_row - self.pos
        }
    }

    fn compact(&mut self) {
        if self.pos == 0 {
            return;
        }

        let start_cell = self.pos * self.cells_per_row;
        if start_cell >= self.ends.len() {
            self.data.clear();
            self.ends.clear();
            self.deleted.clear();
            self.pos = 0;
            return;
        }

        let start = self.ends[start_cell - 1] as usize;
        let consumed = self.pos;
        self.data.drain(..start);
        self.ends.drain(..start_cell);
        self.deleted.drain(..consumed);
        for end in &mut self.ends {
            *end -= start as u32;
        }
        self.pos = 0;
    }

    fn fill_entries(&mut self, rows: &[(&Vec<u8>, &Option<Vec<Vec<u8>>>)]) {
        for (key, cells) in rows {
            match cells {
                Some(cells) => {
                    for cell in cells {
                        self.data.extend_from_slice(cell);
                        self.ends.push(self.data.len() as u32);
                    }
                    self.deleted.push(false);
                }
                None => {
                    self.data.extend_from_slice(key);
                    self.ends.push(self.data.len() as u32);
                    for _ in 1..self.cells_per_row {
                        self.ends.push(self.data.len() as u32);
                    }
                    self.deleted.push(true);
                }
            }
        }
    }

    fn is_deleted(&self, row: usize) -> bool {
        self.deleted.get(row).copied().unwrap_or(false)
    }

    fn current_key(&self) -> Vec<u8> {
        self.get(self.pos, 0).to_vec()
    }

    fn fill(&mut self, batches: &[Batch<'_>]) {
        let rows = batches.first().map(|batch| batch.len()).unwrap_or(0);
        for row in 0..rows {
            for batch in batches {
                let value = batch.get(row);
                self.data.extend_from_slice(value);
                self.ends.push(self.data.len() as u32);
            }
            self.deleted.push(false);
        }
    }

    fn get(&self, row: usize, cell: usize) -> &[u8] {
        let index = row * self.cells_per_row + cell;
        let start = if index == 0 {
            0
        } else {
            self.ends[index - 1] as usize
        };
        let end = self.ends[index] as usize;
        &self.data[start..end]
    }
}

/// Owned row-major copy of values from every table.
///
/// Cell `t` of a row is column `t` in table order: all columns of table 0, then table 1, and so on.
pub struct InterleavedBatch {
    data: Vec<u8>,
    ends: Vec<u32>,
    cells_per_row: usize,
}

impl InterleavedBatch {
    pub fn len(&self) -> usize {
        if self.cells_per_row == 0 {
            0
        } else {
            self.ends.len() / self.cells_per_row
        }
    }

    pub fn cells_per_row(&self) -> usize {
        self.cells_per_row
    }

    pub fn get(&self, row: usize, cell: usize) -> &[u8] {
        let index = row * self.cells_per_row + cell;
        let start = if index == 0 {
            0
        } else {
            self.ends[index - 1] as usize
        };
        let end = self.ends[index] as usize;
        &self.data[start..end]
    }
}

pub struct MergedTableCursor<'a> {
    cursors: Vec<TableCursor>,
    pending: Vec<PendingRows>,
    exhausted: Vec<bool>,
    cells_per_row: usize,
    /// When set, sources are the memtable and then flushed tables, newest first.
    /// Rows are merged by column 0, and the newest source wins ties.
    key_merge: bool,
    seek_key: Option<Vec<u8>>,
    memtable: Option<&'a MemTable>,
    mem_cursor: Option<BtreeCursor<'a>>,
    masks: Vec<DeleteMask>,
    row_indexes: Vec<RowIndex>,
}

impl MergedTableCursor<'static> {
    pub async fn seek_to_key(tables: &[(TableReader, &[ColumnId])], key: &[u8]) -> Self {
        let mut cursors = Vec::with_capacity(tables.len());
        for (table, column_ids) in tables {
            cursors.push(TableCursor::seek_to_key(table.clone(), key, column_ids).await);
        }
        Self::from_cursors(cursors, tables)
    }

    pub async fn seek_to_row_index(
        tables: &[(TableReader, &[ColumnId])],
        row_index: u32,
    ) -> Self {
        let mut cursors = Vec::with_capacity(tables.len());
        for (table, column_ids) in tables {
            cursors.push(
                TableCursor::seek_to_row_index(table.clone(), row_index, column_ids).await,
            );
        }
        Self::from_cursors(cursors, tables)
    }

    fn from_cursors(
        cursors: Vec<TableCursor>,
        tables: &[(TableReader, &[ColumnId])],
    ) -> MergedTableCursor<'static> {
        let column_counts: Vec<usize> = tables.iter().map(|(_, columns)| columns.len()).collect();
        let cells_per_row = column_counts.iter().sum();
        let pending = column_counts
            .into_iter()
            .map(PendingRows::new)
            .collect();

        Self {
            cursors,
            pending,
            exhausted: vec![false; tables.len()],
            cells_per_row,
            key_merge: false,
            seek_key: None,
            memtable: None,
            mem_cursor: None,
            masks: Vec::new(),
            row_indexes: Vec::new(),
        }
    }
}

impl<'a> MergedTableCursor<'a> {
    pub async fn from_memtable(memtable: &'a MemTable) -> Self {
        Self::open_memtable(memtable, None).await
    }

    pub async fn seek_memtable(memtable: &'a MemTable, key: &[u8]) -> Self {
        Self::open_memtable(memtable, Some(key)).await
    }

    async fn open_memtable(memtable: &'a MemTable, seek_key: Option<&[u8]>) -> Self {
        let column_ids = memtable.column_ids();
        let mem_cursor = memtable.cursor(seek_key);

        let mut pending = vec![PendingRows::new(column_ids.len())];
        let mut exhausted = vec![mem_cursor.peek().is_none()];
        let mut cursors = Vec::new();
        let mut masks = Vec::new();
        let mut row_indexes = Vec::new();

        for table_id in memtable.flushed_tables().await {
            let reader = TableReader::new(table_id);
            if column_ids.is_empty() || reader.len(BlockType::Data, column_ids[0]).await == 0 {
                continue;
            }
            let row_index = if let Some(key) = seek_key {
                reader.row_index_by_key(key).await
            } else {
                0
            };
            let cursor = if let Some(key) = seek_key {
                TableCursor::seek_to_key(reader.clone(), key, &column_ids).await
            } else {
                TableCursor::seek_to_row_index(reader.clone(), 0, &column_ids).await
            };
            let rows = crate::delete_mask::table_rows(&reader).await;
            cursors.push(cursor);
            masks.push(DeleteMask::load(table_id, rows).await);
            row_indexes.push(row_index);
            pending.push(PendingRows::new(column_ids.len()));
            exhausted.push(false);
        }

        Self {
            cursors,
            pending,
            exhausted,
            cells_per_row: column_ids.len(),
            key_merge: true,
            seek_key: seek_key.map(|key| key.to_vec()),
            memtable: Some(memtable),
            mem_cursor: Some(mem_cursor),
            masks,
            row_indexes,
        }
    }

    pub async fn next(&mut self, batch_size: u32) -> InterleavedBatch {
        if self.key_merge {
            return self.next_by_key(batch_size).await;
        }

        let batch_rows = batch_size as usize;
        if self.cursors.is_empty() || batch_rows == 0 {
            return InterleavedBatch {
                data: Vec::new(),
                ends: Vec::new(),
                cells_per_row: self.cells_per_row,
            };
        }

        while !self.exhausted.iter().any(|done| *done) {
            let ready = self
                .pending
                .iter()
                .map(|rows| rows.remaining())
                .min()
                .unwrap_or(0);
            if ready >= batch_rows {
                break;
            }

            for index in 0..self.cursors.len() {
                if self.exhausted[index] || self.pending[index].remaining() >= batch_rows {
                    continue;
                }

                let batches = self.cursors[index].next(batch_size).await;
                if batches.first().map(|batch| batch.len()).unwrap_or(0) == 0 {
                    self.exhausted[index] = true;
                } else {
                    self.pending[index].fill(&batches);
                }
            }
        }

        let rows = self
            .pending
            .iter()
            .map(|pending| pending.remaining())
            .min()
            .unwrap_or(0)
            .min(batch_rows);

        let mut data = Vec::new();
        let mut ends = Vec::with_capacity(rows * self.cells_per_row);

        for row in 0..rows {
            for pending in &self.pending {
                let absolute = pending.pos + row;
                for cell in 0..pending.cells_per_row {
                    let value = pending.get(absolute, cell);
                    data.extend_from_slice(value);
                    ends.push(data.len() as u32);
                }
            }
        }

        for pending in &mut self.pending {
            pending.pos += rows;
            pending.compact();
        }

        InterleavedBatch {
            data,
            ends,
            cells_per_row: self.cells_per_row,
        }
    }

    pub async fn has_value(&self, column_id: ColumnId, value: &[u8]) -> bool {
        if self
            .memtable
            .is_some_and(|memtable| memtable.has_value(column_id, value))
        {
            return true;
        }

        for cursor in &self.cursors {
            if cursor.has_value(column_id, value).await {
                return true;
            }
        }
        false
    }

    async fn prepare_source(&mut self, index: usize, batch_size: u32) {
        loop {
            if self.exhausted[index] {
                return;
            }

            if self.pending[index].remaining() == 0 {
                if index == 0 {
                    self.exhausted[index] = true;
                    return;
                }
                let batches = self.cursors[index - 1].next(batch_size).await;
                let batch_len = batches.first().map(|batch| batch.len()).unwrap_or(0);
                if batch_len == 0 {
                    self.exhausted[index] = true;
                    return;
                }
                let start = self.row_indexes[index - 1];
                let mask = &self.masks[index - 1];
                let mut kept = Vec::new();
                for row in 0..batch_len {
                    if !mask.is_set(start + row as u32) {
                        kept.push(row);
                    }
                }
                if !kept.is_empty() {
                    let cells = self.cells_per_row;
                    let mut rows = Vec::new();
                    for row in kept {
                        let mut values = Vec::with_capacity(cells);
                        for cell in 0..cells {
                            values.push(batches[cell].get(row).to_vec());
                        }
                        rows.push(values);
                    }
                    let entries: Vec<(Vec<u8>, Option<Vec<Vec<u8>>>)> = rows
                        .into_iter()
                        .map(|values| (values[0].clone(), Some(values)))
                        .collect();
                    let borrowed: Vec<(&Vec<u8>, &Option<Vec<Vec<u8>>>)> =
                        entries.iter().map(|(key, value)| (key, value)).collect();
                    self.pending[index].fill_entries(&borrowed);
                }
                self.row_indexes[index - 1] = start + batch_len as u32;
                continue;
            }

            if let Some(seek) = self.seek_key.clone() {
                let key = self.pending[index].current_key();
                if key.as_slice() < seek.as_slice() {
                    self.pending[index].pos += 1;
                    continue;
                }
            }
            return;
        }
    }

    async fn next_by_key(&mut self, batch_size: u32) -> InterleavedBatch {
        let batch_rows = batch_size as usize;
        if batch_rows == 0 {
            return InterleavedBatch {
                data: Vec::new(),
                ends: Vec::new(),
                cells_per_row: self.cells_per_row,
            };
        }

        let mut data = Vec::new();
        let mut ends = Vec::new();
        let mut emitted = 0;

        while emitted < batch_rows {
            for index in 1..self.pending.len() {
                self.prepare_source(index, batch_size).await;
            }

            let mut winner: Option<usize> = None;
            let mut winner_key = Vec::new();
            if let Some((key, _)) = self.mem_cursor.as_ref().and_then(BtreeCursor::peek) {
                winner = Some(0);
                winner_key = key.as_slice().to_vec();
            }
            for index in 1..self.pending.len() {
                if self.pending[index].remaining() == 0 {
                    continue;
                }
                let key = self.pending[index].current_key();
                let replace = match winner {
                    None => true,
                    Some(best) => {
                        key.as_slice() < winner_key.as_slice()
                            || (key.as_slice() == winner_key.as_slice() && index < best)
                    }
                };
                if replace {
                    winner = Some(index);
                    winner_key = key;
                }
            }

            let Some(winner) = winner else {
                break;
            };

            let deleted = if winner == 0 {
                self.mem_cursor
                    .as_ref()
                    .and_then(BtreeCursor::peek)
                    .is_none_or(|(_, value)| value.is_none())
            } else {
                let row = self.pending[winner].pos;
                self.pending[winner].is_deleted(row)
            };
            if !deleted {
                if winner == 0 {
                    let cells = self
                        .mem_cursor
                        .as_ref()
                        .and_then(BtreeCursor::peek)
                        .and_then(|(_, value)| value.as_ref())
                        .expect("live memtable row");
                    for cell in cells {
                        data.extend_from_slice(cell.as_slice());
                        ends.push(data.len() as u32);
                    }
                } else {
                    let row = self.pending[winner].pos;
                    for cell in 0..self.cells_per_row {
                        let value = self.pending[winner].get(row, cell);
                        data.extend_from_slice(value);
                        ends.push(data.len() as u32);
                    }
                }
                emitted += 1;
            }

            if self
                .mem_cursor
                .as_ref()
                .and_then(BtreeCursor::peek)
                .is_some_and(|(key, _)| key.as_slice() == winner_key.as_slice())
            {
                self.mem_cursor.as_mut().unwrap().advance();
            }
            for pending in &mut self.pending {
                if pending.remaining() > 0 && pending.current_key() == winner_key {
                    pending.pos += 1;
                }
            }
        }

        for pending in &mut self.pending {
            pending.compact();
        }

        InterleavedBatch {
            data,
            ends,
            cells_per_row: self.cells_per_row,
        }
    }
}
