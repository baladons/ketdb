use futures::future::join_all;

use crate::{batch::Batch, block_cursor::BlockCursor, table_reader::TableReader, types::ColumnId};

pub struct TableCursor {
    table: TableReader,
    cursors: Vec<BlockCursor>,
}

impl TableCursor {
    pub async fn seek_to_key(table: TableReader, key: &[u8], column_ids: &[ColumnId]) -> Self {
        let row_index = table.row_index_by_key(key).await;

        Self::seek_to_row_index(table, row_index, column_ids).await
    }

    pub async fn seek_to_row_index(
        table: TableReader,
        row_index: u32,
        column_ids: &[ColumnId],
    ) -> Self {
        let futures = column_ids.iter().map(|&column_id| {
            let table_clone = table.clone();
            async move { BlockCursor::new(table_clone, column_id, row_index).await }
        });

        let cursors = join_all(futures).await;

        Self { table, cursors }
    }

    pub async fn has_value(&self, column_id: ColumnId, value: &[u8]) -> bool {
        self.table.has_value(column_id, value).await
    }

    pub async fn next(&mut self, batch_size: u32) -> Vec<Batch<'_>> {
        let futures = self
            .cursors
            .iter_mut()
            .map(|cursor| cursor.next(batch_size));

        let mut batches = join_all(futures).await;

        let min_len = batches.iter().map(|batch| batch.len()).min().unwrap();

        for batch in batches.iter_mut() {
            batch.truncate(min_len);
        }

        batches
    }
}
