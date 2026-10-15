use rkyv::rancor::{Error, Fallible};
use rkyv::with::{ArchiveWith, DeserializeWith, Map, SerializeWith};
use rkyv::{from_bytes_unchecked, to_bytes, Archive, Archived, Deserialize, Place, Serialize};
use uuid::Uuid;

use crate::{context::Context, types::TableGroupId, virtual_file::VirtualFile};

struct UuidBytes;

impl ArchiveWith<Uuid> for UuidBytes {
    type Archived = Archived<[u8; 16]>;
    type Resolver = [(); 16];

    fn resolve_with(field: &Uuid, resolver: Self::Resolver, out: Place<Self::Archived>) {
        field.into_bytes().resolve(resolver, out);
    }
}

impl<S> SerializeWith<Uuid, S> for UuidBytes
where
    S: Fallible + ?Sized,
    [u8; 16]: Serialize<S>,
{
    fn serialize_with(
        field: &Uuid,
        serializer: &mut S,
    ) -> Result<<UuidBytes as ArchiveWith<Uuid>>::Resolver, S::Error> {
        field.into_bytes().serialize(serializer)
    }
}

impl<D> DeserializeWith<Archived<[u8; 16]>, Uuid, D> for UuidBytes
where
    D: Fallible + ?Sized,
    Archived<[u8; 16]>: Deserialize<[u8; 16], D>,
{
    fn deserialize_with(field: &Archived<[u8; 16]>, deserializer: &mut D) -> Result<Uuid, D::Error> {
        Ok(Uuid::from_bytes(field.deserialize(deserializer)?))
    }
}

#[derive(Archive, Deserialize, Serialize, Debug)]
pub struct TableGroup {
    pub id: TableGroupId,
    pub column_ids: Vec<u32>,
    /// Flushed tables, newest first.
    #[rkyv(with = Map<UuidBytes>)]
    pub tables: Vec<Uuid>,
    pub wal_id: [u8; 16],
    pub deleted_rows: u64,
}

#[derive(Archive, Deserialize, Serialize, Debug, Default)]
pub struct Metadata {
    pub groups: Vec<TableGroup>,
}

impl Metadata {
    pub fn group(&self, id: TableGroupId) -> &TableGroup {
        self.groups
            .iter()
            .find(|group| group.id == id)
            .expect("missing table")
    }

    pub fn group_mut(&mut self, id: TableGroupId) -> &mut TableGroup {
        self.groups
            .iter_mut()
            .find(|group| group.id == id)
            .expect("missing table")
    }

    pub fn ensure_group(&mut self, id: TableGroupId, column_ids: Vec<u32>) {
        if self.groups.iter().any(|group| group.id == id) {
            return;
        }
        self.groups.push(TableGroup {
            id,
            column_ids,
            tables: Vec::new(),
            wal_id: [0; 16],
            deleted_rows: 0,
        });
    }

    pub fn remove_group(&mut self, id: TableGroupId) -> TableGroup {
        let index = self
            .groups
            .iter()
            .position(|group| group.id == id)
            .expect("missing table");
        self.groups.remove(index)
    }
}

pub async fn load() -> &'static mut Metadata {
    if Context::current().metadata.is_none() {
        let metadata = read_file().await;
        Context::current().metadata = Some(metadata);
    }
    Context::current().metadata.as_mut().unwrap()
}

async fn read_file() -> Metadata {
    let file = VirtualFile::metadata();
    let len = file.len().await;
    if len < 8 {
        return Metadata::default();
    }

    let prefix = file.read(8, 0).await;
    let body_len = u64::from_le_bytes(prefix.try_into().unwrap()) as usize;
    if body_len == 0 {
        return Metadata::default();
    }

    let bytes = file.read(body_len, 8).await;
    unsafe { from_bytes_unchecked::<Metadata, Error>(&bytes).unwrap() }
}

pub async fn store() {
    let body = to_bytes::<Error>(Context::current().metadata.as_ref().expect("metadata")).unwrap();
    let mut bytes = Vec::with_capacity(8 + body.len());
    bytes.extend_from_slice(&(body.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&body);
    VirtualFile::metadata().write_at(0, &bytes).await;
}
