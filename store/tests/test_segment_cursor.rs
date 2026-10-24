use std::fs;
use std::sync::Mutex;

use store::{
    binary_search::SearchResult,
    cursor::{ScanCursor, SegmentCursor},
    file_id::FileId,
    segment::Segment,
    writer::{FrameWriter, SegmentWriter},
    context::Context,
};

static DATA_DIR_LOCK: Mutex<()> = Mutex::new(());

struct DataWorkspace {
    _lock: std::sync::MutexGuard<'static, ()>,
    _dir: tempfile::TempDir,
}

fn chdir_to_data_workspace() -> DataWorkspace {
    let lock = DATA_DIR_LOCK.lock().expect("lock");
    let dir = tempfile::tempdir().expect("tempdir");
    std::env::set_current_dir(dir.path()).expect("chdir");
    fs::create_dir_all("data").expect("mkdir data");
    DataWorkspace {
        _lock: lock,
        _dir: dir,
    }
}

async fn write_segment_keys(
    context: &mut Context,
    threshold: usize,
    keys: &[&[u8]],
) -> Segment {
    let file_id = FileId::new_segment();
    let mut frame_writer = FrameWriter::open(file_id).await.expect("open frame writer");
    let mut segment_writer = SegmentWriter::new(file_id, threshold);

    for key in keys {
        segment_writer
            .write(&mut frame_writer, context, 0, key)
            .await
            .expect("write key");
    }

    let file_id = segment_writer
        .finish(&mut frame_writer, context)
        .await
        .expect("flush segment");

    Segment::open(context, file_id).await.expect("open segment")
}

async fn write_segment(context: &mut Context, threshold: usize) -> Segment {
    let file_id = FileId::new_segment();
    let mut frame_writer = FrameWriter::open(file_id).await.expect("open frame writer");
    let mut segment_writer = SegmentWriter::new(file_id, threshold);

    let rows: [(&[u8], &[u8]); 3] = [(b"a", b"v1"), (b"c", b"v2"), (b"e", b"v3")];
    for (key, value) in rows {
        segment_writer
            .write(&mut frame_writer, context, 0, key)
            .await
            .expect("write key");
        segment_writer
            .write(&mut frame_writer, context, 1, value)
            .await
            .expect("write value");
    }

    let file_id = segment_writer
        .finish(&mut frame_writer, context)
        .await
        .expect("flush segment");

    Segment::open(context, file_id).await.expect("open segment")
}

#[compio::test]
async fn segment_writer_flush_produces_searchable_segment() {
    let _workspace = chdir_to_data_workspace();
    let mut context = Context::new(16, 8 * 1024 * 1024);
    let segment = write_segment(&mut context, 1024).await;

    assert_eq!(
        segment.search_row_index(&mut context, b"a").await.expect("search"),
        SearchResult::Found(0)
    );
    assert_eq!(
        segment.search_row_index(&mut context, b"c").await.expect("search"),
        SearchResult::Found(1)
    );
    assert_eq!(
        segment.search_row_index(&mut context, b"e").await.expect("search"),
        SearchResult::Found(2)
    );
}

#[compio::test]
async fn cursor_seek_first_loads_aligned_columns() {
    let _workspace = chdir_to_data_workspace();
    let mut context = Context::new(16, 8 * 1024 * 1024);
    let segment = write_segment(&mut context, 1024).await;

    let cursor = SegmentCursor::seek_first(segment, vec![0, 1], &mut context)
        .await
        .expect("seek_first");

    assert_eq!(cursor.row_index(), 0);
    assert_eq!(cursor.column_ids(), &[0, 1]);
    assert_eq!(cursor.blocks().len(), 2);
    assert_eq!(cursor.blocks()[0].get(0), b"a");
    assert_eq!(cursor.blocks()[0].get(1), b"c");
    assert_eq!(cursor.blocks()[0].get(2), b"e");
    assert_eq!(cursor.blocks()[1].get(0), b"v1");
    assert_eq!(cursor.blocks()[1].get(2), b"v3");
}

#[compio::test]
async fn cursor_seek_key_finds_row_block() {
    let _workspace = chdir_to_data_workspace();
    let mut context = Context::new(16, 8 * 1024 * 1024);
    let segment = write_segment(&mut context, 1024).await;

    let cursor = SegmentCursor::seek_key(segment, vec![0, 1], b"c", &mut context)
        .await
        .expect("seek_key");

    assert_eq!(cursor.row_index(), 1);
    assert_eq!(cursor.blocks()[0].get(1), b"c");
    assert_eq!(cursor.blocks()[1].get(1), b"v2");

    let batches = cursor.batch_read();
    assert_eq!(batches.len(), 2);
    assert_eq!(batches[0].len(), 2);
    assert_eq!(batches[1].len(), 2);
}

#[compio::test]
async fn cursor_seek_key_across_data_blocks() {
    let _workspace = chdir_to_data_workspace();
    let mut context = Context::new(16, 8 * 1024 * 1024);
    // Flush after each key once the block exceeds one byte (keys `a`+`c`, then `e`).
    let segment = write_segment(&mut context, 1).await;

    assert_eq!(
        segment.search_row_index(&mut context, b"a").await.expect("search"),
        SearchResult::Found(0)
    );
    assert_eq!(
        segment.search_row_index(&mut context, b"c").await.expect("search"),
        SearchResult::Found(1)
    );
    assert_eq!(
        segment.search_row_index(&mut context, b"e").await.expect("search"),
        SearchResult::Found(2)
    );

    let cursor = SegmentCursor::seek_key(segment, vec![0], b"e", &mut context)
        .await
        .expect("seek_key");

    assert_eq!(cursor.row_index(), 2);
    assert_eq!(cursor.blocks()[0].row_count(), 1);
    assert_eq!(cursor.blocks()[0].get(0), b"e");
}

#[compio::test]
async fn scan_cursor_opens_first_segment_from_disk() {
    let _workspace = chdir_to_data_workspace();
    let mut context = Context::new(16, 8 * 1024 * 1024);
    let segment = write_segment(&mut context, 1024).await;
    let file_id = segment.file_id();

    let cursor = ScanCursor::seek_first(vec![file_id], vec![0, 1], &mut context)
        .await
        .expect("scan seek_first");

    assert_eq!(cursor.current_file_id(), file_id);
    assert_eq!(cursor.segment_index(), 0);
    assert_eq!(cursor.blocks()[0].get(0), b"a");
    assert_eq!(cursor.batch_read()[1].len(), 3);
}

#[compio::test]
async fn scan_cursor_advances_across_segments() {
    let _workspace = chdir_to_data_workspace();
    let mut context = Context::new(16, 8 * 1024 * 1024);

    let seg1 = write_segment_keys(&mut context, 0, &[b"a", b"c"]).await;
    let seg2 = write_segment_keys(&mut context, 0, &[b"g", b"i"]).await;

    let mut cursor = ScanCursor::seek_first(
        vec![seg1.file_id(), seg2.file_id()],
        vec![0],
        &mut context,
    )
    .await
    .expect("seek_first");

    assert_eq!(cursor.segment_index(), 0);
    assert_eq!(cursor.blocks()[0].get(0), b"a");

    assert_eq!(cursor.advance(&mut context, 1).await.expect("advance"), 0);
    assert_eq!(cursor.segment_index(), 0);
    assert_eq!(cursor.row_index(), 1);
    assert_eq!(cursor.blocks()[0].get(0), b"c");

    assert_eq!(cursor.advance(&mut context, 1).await.expect("advance"), 0);
    assert_eq!(cursor.segment_index(), 1);
    assert_eq!(cursor.row_index(), 0);
    assert_eq!(cursor.blocks()[0].get(0), b"g");

    assert_eq!(cursor.advance(&mut context, 1).await.expect("advance"), 0);
    assert_eq!(cursor.segment_index(), 1);
    assert_eq!(cursor.blocks()[0].get(0), b"i");

    assert_eq!(cursor.advance(&mut context, 1).await.expect("advance"), 1);
}
