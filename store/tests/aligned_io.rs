use std::fs;

use compio::runtime::Runtime;
use format::{aligned_vec::AlignedVec, context::Context, virtual_file::VirtualFile};
use uuid::Uuid;

#[test]
fn virtual_file_append_and_read_roundtrip() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    std::env::set_current_dir(manifest_dir).unwrap();
    fs::create_dir_all("data").unwrap();

    let table_id = Uuid::from_u128(1);
    let file_path = format!("data/{}.ket", table_id);
    let _ = fs::remove_file(&file_path);
    fs::File::create(&file_path).unwrap();

    Context::new(8).into_current();

    Runtime::new().unwrap().block_on(async {
        let file = VirtualFile::new(table_id);
        file.append(b"hello").await;
        file.append(b"world").await;
        assert_eq!(file.read(5, 0).await, b"hello");
        assert_eq!(file.read(5, 4096).await, b"world");
    });

    let _ = fs::remove_file(&file_path);
}

#[test]
fn aligned_vec_is_page_aligned() {
    let buffer = AlignedVec::from_slice(b"test");
    let ptr = buffer.as_slice().as_ptr() as usize;
    assert_eq!(ptr % 4096, 0);
    assert_eq!(buffer.logical_len(), 4);
    assert_eq!(buffer.io_len(), 4096);
}
