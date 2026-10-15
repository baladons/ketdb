use std::fs::File;
use std::os::fd::AsRawFd;

/// Serializes tests that share `data/metadata.ket`.
pub struct DataLock {
    file: File,
}

impl DataLock {
    pub fn acquire() -> Self {
        std::env::set_current_dir(env!("CARGO_MANIFEST_DIR")).unwrap();
        std::fs::create_dir_all("data").unwrap();
        let file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open("data/.test.lock")
            .unwrap();
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        assert_eq!(rc, 0);

        format::virtual_file::VirtualFile::remove_metadata();
        format::context::Context::new(8).into_current();
        Self { file }
    }
}

impl Drop for DataLock {
    fn drop(&mut self) {
        format::virtual_file::VirtualFile::remove_metadata();
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}
