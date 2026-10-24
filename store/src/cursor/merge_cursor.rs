use crate::{
    batch::Batch, block::Block, context::Context, error::Error, file_id::FileId, segment::Segment,
};

use super::{block_cursor::BlockCursor, segment_cursor::SegmentCursor};

pub struct MergeCursor {
    column_ids: Vec<u32>,
    cursors: Vec<SegmentCursor>,
}

impl MergeCursor {
    pub async fn seek_first(
        file_ids: Vec<FileId>,
        column_ids: Vec<u32>,
        context: &mut Context,
    ) -> Result<Self, Error> {
        if !column_ids.contains(&0) {
            return Err(Error::DeserializationFailure);
        }

        let mut cursors = Vec::with_capacity(file_ids.len());
        for file_id in file_ids {
            let segment = Segment::open(context, file_id).await?;
            cursors.push(SegmentCursor::seek_first(segment, column_ids.clone(), context).await?);
        }

        Ok(Self {
            column_ids,
            cursors,
        })
    }

    /// Advance `count` merged rows. Returns how many could not be advanced.
    pub async fn advance(&mut self, context: &mut Context, count: u32) -> Result<u32, Error> {
        let mut remaining = count;
        while remaining > 0 {
            let Some(index) = self.winner() else {
                return Ok(remaining);
            };

            self.cursors[index].advance(context, 1).await?;
            if is_exhausted(&self.cursors[index]) {
                self.cursors.swap_remove(index);
            }
            remaining -= 1;
        }
        Ok(0)
    }

    pub fn column_ids(&self) -> &[u32] {
        &self.column_ids
    }

    pub fn blocks(&self) -> Vec<&Block> {
        self.winner()
            .map(|index| self.cursors[index].blocks())
            .unwrap_or_default()
    }

    pub fn batch(&self) -> Vec<Batch<'_>> {
        let Some(winner) = self.winner() else {
            return Vec::new();
        };
        self.cursors[winner]
            .batch()
            .into_iter()
            .map(|batch| batch.truncate(1))
            .collect()
    }

    fn winner(&self) -> Option<usize> {
        self.cursors
            .iter()
            .enumerate()
            .min_by(|a, b| current_key(a.1).cmp(current_key(b.1)).then(a.0.cmp(&b.0)))
            .map(|(index, _)| index)
    }
}

fn key_cursor(cursor: &SegmentCursor) -> &BlockCursor {
    cursor
        .block_cursors()
        .iter()
        .find(|cursor| cursor.column_id() == 0)
        .expect("column 0")
}

fn current_key(cursor: &SegmentCursor) -> &[u8] {
    let key_cursor = key_cursor(cursor);
    key_cursor.block().get(key_cursor.index_in_block())
}

fn is_exhausted(cursor: &SegmentCursor) -> bool {
    cursor
        .block_cursors()
        .first()
        .is_some_and(|cursor| cursor.index_in_block() >= cursor.block().row_count())
}
