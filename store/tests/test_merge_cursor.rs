use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::os::fd::AsRawFd;
use std::sync::Mutex;

use store::{
    context::Context,
    cursor::MergeCursor,
    file_id::FileId,
    writer::{FrameWriter, SegmentWriter},
};

static DATA_DIR_LOCK: Mutex<()> = Mutex::new(());

const ROW_COUNT: u32 = 10_000;
const SEGMENT_COUNT: u32 = 4;

struct DataWorkspace {
    _dir: tempfile::TempDir,
    _lock: std::sync::MutexGuard<'static, ()>,
    _file_lock: File,
}

fn chdir_to_data_workspace() -> DataWorkspace {
    let lock = DATA_DIR_LOCK.lock().expect("lock");
    let file_lock = lock_data_dir();
    let dir = tempfile::tempdir().expect("tempdir");
    std::env::set_current_dir(dir.path()).expect("chdir");
    fs::create_dir_all("data").expect("mkdir data");
    DataWorkspace {
        _dir: dir,
        _lock: lock,
        _file_lock: file_lock,
    }
}

fn lock_data_dir() -> File {
    let file = File::options()
        .create(true)
        .write(true)
        .open("/tmp/ketdb-store-tests.lock")
        .expect("lock file");
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    assert_eq!(rc, 0, "flock");
    file
}

fn key_bytes(key: u32) -> [u8; 4] {
    key.to_be_bytes()
}

fn value_hash(key: &[u8]) -> [u8; 8] {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish().to_le_bytes()
}

async fn write_striped_segments(context: &mut Context) -> Vec<FileId> {
    let mut writers = Vec::with_capacity(SEGMENT_COUNT as usize);
    for _ in 0..SEGMENT_COUNT {
        let file_id = FileId::new_segment();
        let frame_writer = FrameWriter::open(file_id).await.expect("open frame writer");
        let segment_writer = SegmentWriter::new(file_id, 4 * 1024);
        writers.push((frame_writer, segment_writer));
    }

    for key in 0..ROW_COUNT {
        let index = (key % SEGMENT_COUNT) as usize;
        let (frame_writer, segment_writer) = &mut writers[index];
        let key = key_bytes(key);
        let value = value_hash(&key);
        segment_writer
            .write(frame_writer, context, 0, &key)
            .await
            .expect("write key");
        segment_writer
            .write(frame_writer, context, 1, &value)
            .await
            .expect("write value");
    }

    let mut file_ids = Vec::with_capacity(writers.len());
    for (mut frame_writer, segment_writer) in writers {
        let file_id = segment_writer
            .finish(&mut frame_writer, context)
            .await
            .expect("flush segment");
        file_ids.push(file_id);
    }
    file_ids
}

async fn assert_merged_rows(cursor: &mut MergeCursor, context: &mut Context, start: u32, end: u32) {
    for key in start..end {
        assert!(!cursor.done(), "cursor finished before key {key}");
        let batch = cursor.batch();
        let key_bytes = key_bytes(key);
        assert_eq!(batch.len(), 2);
        assert_eq!(&batch[0][0], &key_bytes);
        assert_eq!(&batch[1][0], &value_hash(&key_bytes));

        let unread = cursor.advance(context, 1).await.expect("advance");
        if key + 1 == end {
            assert_eq!(unread, 1);
            assert!(cursor.done());
        } else {
            assert_eq!(unread, 0);
        }
    }
}

#[compio::test]
async fn merge_cursor_seeks_striped_integer_keys() {
    let _workspace = chdir_to_data_workspace();
    let mut context = Context::new(64, 8 * 1024 * 1024);
    let file_ids = write_striped_segments(&mut context).await;
    let columns = vec![0, 1];

    let mut cursor = MergeCursor::seek_first(file_ids.clone(), columns.clone(), &mut context)
        .await
        .expect("seek_first");
    assert_merged_rows(&mut cursor, &mut context, 0, ROW_COUNT).await;

    let mut cursor = MergeCursor::seek_key(
        file_ids.clone(),
        columns.clone(),
        &key_bytes(0),
        &mut context,
    )
    .await
    .expect("seek_key 0");
    assert_merged_rows(&mut cursor, &mut context, 0, ROW_COUNT).await;

    let mut cursor = MergeCursor::seek_key(
        file_ids.clone(),
        columns.clone(),
        &key_bytes(100),
        &mut context,
    )
    .await
    .expect("seek_key 100");
    assert_merged_rows(&mut cursor, &mut context, 100, ROW_COUNT).await;

    let last = ROW_COUNT - 1;
    let mut cursor = MergeCursor::seek_key(
        file_ids.clone(),
        columns.clone(),
        &key_bytes(last),
        &mut context,
    )
    .await
    .expect("seek_key last");
    assert_merged_rows(&mut cursor, &mut context, last, ROW_COUNT).await;

    let mut cursor = MergeCursor::seek_key(file_ids, columns, &key_bytes(ROW_COUNT), &mut context)
        .await
        .expect("seek_key past end");
    assert!(cursor.done());
    assert!(cursor.batch().is_empty());
    assert_eq!(cursor.advance(&mut context, 1).await.expect("advance"), 1);
}
