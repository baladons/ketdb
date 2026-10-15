use uuid::Uuid;

use crate::{aligned_vec::PAGE_SIZE, virtual_file::VirtualFile};

pub struct Wal {
    id: Uuid,
    page_index: usize,
    buffer: Vec<u8>,
}

impl Wal {
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub async fn open(id: Uuid) -> (Self, Vec<(Vec<u8>, Option<Vec<Vec<u8>>>)>) {
        let file = VirtualFile::ensure(id);
        let len = file.len().await;
        if len == 0 {
            return (
                Self {
                    id,
                    page_index: 0,
                    buffer: Vec::new(),
                },
                Vec::new(),
            );
        }

        let mut records = Vec::new();
        let pages = len / PAGE_SIZE;
        for page_index in 0..pages {
            let page = file.read(PAGE_SIZE, page_index * PAGE_SIZE).await;
            let used = page_used(&page);
            records.extend(parse_page(&page[..used]));
            if page_index + 1 == pages {
                return (
                    Self {
                        id,
                        page_index,
                        buffer: page[..used].to_vec(),
                    },
                    records,
                );
            }
        }

        (
            Self {
                id,
                page_index: 0,
                buffer: Vec::new(),
            },
            records,
        )
    }

    pub async fn append(&mut self, values: &[&[u8]], buffered_journal: bool) {
        let record = upsert_record(values);
        self.append_record(&record, buffered_journal).await;
    }

    pub(crate) async fn append_encoded(&mut self, record: &[u8], buffered_journal: bool) {
        self.append_record(record, buffered_journal).await;
    }

    pub async fn append_delete(&mut self, key: &[u8], buffered_journal: bool) {
        let mut record = vec![0];
        record.extend_from_slice(&(key.len() as u32).to_le_bytes());
        record.extend_from_slice(key);
        self.append_record(&record, buffered_journal).await;
    }

    async fn append_record(&mut self, record: &[u8], buffered_journal: bool) {
        let needed = 4 + record.len();
        assert!(needed <= PAGE_SIZE, "wal record exceeds a page");

        if self.buffer.len() + needed > PAGE_SIZE {
            if buffered_journal {
                self.write_page().await;
            }
            self.page_index += 1;
            self.buffer.clear();
        }

        self.buffer
            .extend_from_slice(&(record.len() as u32).to_le_bytes());
        self.buffer.extend_from_slice(record);
        if !buffered_journal || self.buffer.len() >= PAGE_SIZE {
            self.write_page().await;
        }
    }

    async fn write_page(&self) {
        let file = VirtualFile::ensure(self.id);
        file.write_at((self.page_index * PAGE_SIZE) as u64, &self.buffer)
            .await;
    }
}

pub(crate) fn upsert_record(values: &[&[u8]]) -> Vec<u8> {
    let mut record = vec![1];
    record.extend(encode_row(values));
    record
}

fn encode_row(values: &[&[u8]]) -> Vec<u8> {
    let mut record = Vec::new();
    record.extend_from_slice(&(values.len() as u32).to_le_bytes());
    for value in values {
        record.extend_from_slice(&(value.len() as u32).to_le_bytes());
        record.extend_from_slice(value);
    }
    record
}

fn page_used(page: &[u8]) -> usize {
    let mut offset = 0;
    while offset + 4 <= page.len() {
        let len = u32::from_le_bytes(page[offset..offset + 4].try_into().unwrap()) as usize;
        if len == 0 || offset + 4 + len > page.len() {
            break;
        }
        offset += 4 + len;
    }
    offset
}

fn parse_page(page: &[u8]) -> Vec<(Vec<u8>, Option<Vec<Vec<u8>>>)> {
    let mut offset = 0;
    let mut rows = Vec::new();
    while offset + 4 <= page.len() {
        let len = u32::from_le_bytes(page[offset..offset + 4].try_into().unwrap()) as usize;
        if len == 0 || offset + 4 + len > page.len() {
            break;
        }
        offset += 4;
        rows.push(decode_record(&page[offset..offset + len]));
        offset += len;
    }
    rows
}

fn decode_record(record: &[u8]) -> (Vec<u8>, Option<Vec<Vec<u8>>>) {
    match record[0] {
        0 => {
            let len = u32::from_le_bytes(record[1..5].try_into().unwrap()) as usize;
            (record[5..5 + len].to_vec(), None)
        }
        _ => {
            let values = decode_row(&record[1..]);
            let key = values[0].clone();
            (key, Some(values))
        }
    }
}

fn decode_row(record: &[u8]) -> Vec<Vec<u8>> {
    let count = u32::from_le_bytes(record[..4].try_into().unwrap()) as usize;
    let mut offset = 4;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let len = u32::from_le_bytes(record[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        values.push(record[offset..offset + len].to_vec());
        offset += len;
    }
    values
}
