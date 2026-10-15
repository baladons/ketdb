mod common;

use std::future::Future;

use compio::runtime::Runtime;
use format::{
    metadata,
    store::{Store, WriteConcern},
};

const FLUSH: WriteConcern = WriteConcern {
    buffered_journal: false,
};

fn run(future: impl Future<Output = ()>) {
    let _lock = common::DataLock::acquire();
    Runtime::new().unwrap().block_on(future);
}

#[test]
fn insert_keeps_other_columns_and_update_skips_missing_rows() {
    run(async {
        let mut store = Store::open().await;
        store.create_table(1, &[2, 1]).await;
        assert_eq!(metadata::load().await.group(1).column_ids, vec![0, 2, 1]);

        store.insert(1, &[0, 1], &[b"row", b"one"], FLUSH).await;
        store.flush(1).await;
        assert_eq!(
            store.read(1, &[2, 1, 0], b"row").await.map(|row| row.to_vec()),
            Some(vec![Vec::new(), b"one".to_vec(), b"row".to_vec()])
        );

        store.insert(1, &[0, 2], &[b"row", b"two"], FLUSH).await;
        store.flush(1).await;
        assert_eq!(
            store.read(1, &[1, 2], b"row").await.map(|row| row.to_vec()),
            Some(vec![b"one".to_vec(), b"two".to_vec()])
        );

        store.update(1, &[0, 1], &[b"row", b"ONE"], FLUSH).await;
        store.flush(1).await;
        assert_eq!(
            store.read(1, &[1, 2], b"row").await.map(|row| row.to_vec()),
            Some(vec![b"ONE".to_vec(), b"two".to_vec()])
        );

        store.update(1, &[0, 1], &[b"nope", b"z"], FLUSH).await;
        assert!(store.read(1, &[1], b"nope").await.is_none());
        assert_eq!(
            store.read(1, &[2], b"row").await.map(|row| row.to_vec()),
            Some(vec![b"two".to_vec()])
        );
    });
}

#[test]
fn delete_clears_columns_until_the_key_removes_the_row() {
    run(async {
        let mut store = Store::open().await;
        store.create_table(3, &[0, 1, 2]).await;
        store
            .insert(3, &[0, 1, 2], &[b"k", b"a", b"b"], FLUSH)
            .await;
        store.flush(3).await;

        store.delete(3, &[], b"k", FLUSH).await;
        assert_eq!(
            store.read(3, &[1, 2], b"k").await.map(|row| row.to_vec()),
            Some(vec![b"a".to_vec(), b"b".to_vec()])
        );

        store.delete(3, &[1], b"missing", FLUSH).await;
        store.delete(3, &[1], b"k", FLUSH).await;
        store.flush(3).await;
        assert_eq!(
            store.read(3, &[0, 1, 2], b"k").await.map(|row| row.to_vec()),
            Some(vec![b"k".to_vec(), Vec::new(), b"b".to_vec()])
        );

        store.delete(3, &[0, 2], b"k", FLUSH).await;
        store.flush(3).await;
        assert!(store.read(3, &[0], b"k").await.is_none());
    });
}

#[test]
fn groups_stay_isolated_and_survive_reopen() {
    run(async {
        {
            let mut store = Store::open().await;
            store.create_table(1, &[0, 1]).await;
            store.create_table(2, &[0, 1]).await;
            store.insert(1, &[0, 1], &[b"a", b"from-1"], FLUSH).await;
            store.insert(2, &[0, 1], &[b"a", b"from-2"], FLUSH).await;
            store.insert(2, &[0, 1], &[b"b", b"second"], FLUSH).await;
            store.flush(2).await;
            store.delete_table(1).await;
        }

        let mut store = Store::open().await;
        let mut groups: Vec<_> = metadata::load()
            .await
            .groups
            .iter()
            .map(|group| group.id)
            .collect();
        groups.sort();
        assert_eq!(groups, vec![2]);
        assert_eq!(
            store.read(2, &[1], b"a").await.map(|row| row.to_vec()),
            Some(vec![b"from-2".to_vec()])
        );
        assert_eq!(
            store.read(2, &[1], b"b").await.map(|row| row.to_vec()),
            Some(vec![b"second".to_vec()])
        );

        store.create_table(1, &[0, 1]).await;
        assert!(store.read(1, &[1], b"a").await.is_none());
        store.insert(1, &[0, 1], &[b"a", b"fresh"], FLUSH).await;
        assert_eq!(
            store.read(1, &[1], b"a").await.map(|row| row.to_vec()),
            Some(vec![b"fresh".to_vec()])
        );
        assert_eq!(
            store.read(2, &[1], b"a").await.map(|row| row.to_vec()),
            Some(vec![b"from-2".to_vec()])
        );
    });
}

#[test]
fn column_and_row_deletes_replay_from_the_wal() {
    run(async {
        {
            let mut store = Store::open().await;
            store.create_table(4, &[0, 1, 2]).await;
            store
                .insert(4, &[0, 1, 2], &[b"a", b"1", b"2"], FLUSH)
                .await;
            store
                .insert(4, &[0, 1, 2], &[b"b", b"3", b"4"], FLUSH)
                .await;
            store.delete(4, &[2], b"a", FLUSH).await;
            store.delete(4, &[0], b"b", FLUSH).await;
        }

        let store = Store::open().await;
        assert_eq!(
            store.read(4, &[1, 2], b"a").await.map(|row| row.to_vec()),
            Some(vec![b"1".to_vec(), Vec::new()])
        );
        assert!(store.read(4, &[1], b"b").await.is_none());
    });
}

#[test]
fn buffered_journal_flushes_when_the_page_fills() {
    const BUFFERED: WriteConcern = WriteConcern {
        buffered_journal: true,
    };
    // Two 8-byte cells plus framing is 33 bytes, so 124 records fill a page and the 125th stays buffered.
    const ROWS: u64 = 125;

    run(async {
        {
            let mut store = Store::open().await;
            store.create_table(5, &[0, 1]).await;
            let key = 0u64.to_be_bytes();
            store.insert(5, &[0, 1], &[&key, &key], BUFFERED).await;
        }
        {
            let store = Store::open().await;
            let key = 0u64.to_be_bytes();
            assert!(store.read(5, &[1], &key).await.is_none());
        }

        {
            let mut store = Store::open().await;
            for key in 0..ROWS {
                let bytes = key.to_be_bytes();
                store.insert(5, &[0, 1], &[&bytes, &bytes], BUFFERED).await;
            }
        }

        let store = Store::open().await;
        let first = 0u64.to_be_bytes();
        let last_flushed = 123u64.to_be_bytes();
        let buffered = 124u64.to_be_bytes();
        assert_eq!(
            store.read(5, &[1], &first).await.map(|row| row.to_vec()),
            Some(vec![first.to_vec()])
        );
        assert_eq!(
            store.read(5, &[1], &last_flushed).await.map(|row| row.to_vec()),
            Some(vec![last_flushed.to_vec()])
        );
        assert!(store.read(5, &[1], &buffered).await.is_none());
    });
}
