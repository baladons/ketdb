use crate::{
    buffer::Buffer,
    dtos::{Encoding, Frame},
    error::Error,
    file_id::FileId,
};

use super::{
    buffer_pool::BufferPool,
    encoding_utils::{compress_into, decompress_into, decrypt_in_place, encrypt_in_place},
    key_manager::KeyManager,
};

pub struct Encoder;

impl Encoder {
    pub const fn new() -> Self {
        Self
    }

    pub async fn decode(
        &self,
        pool: &mut BufferPool,
        keys: &mut KeyManager,
        id: FileId,
        frame: &Frame,
        encoded: Buffer,
    ) -> Result<Buffer, Error> {
        let mut encoded = encoded;

        if frame.encoding.encrypted {
            let key = keys.get_encryption_key(id).await;
            decrypt_in_place(key, &mut encoded)?;
        }

        let mut buffer = Buffer::with_capacity(frame.uncompressed_size as usize);

        if frame.encoding.compression > 0 {
            buffer.resize(frame.uncompressed_size as usize, 0);
            decompress_into(&encoded, &mut buffer)?;
        } else {
            buffer.extend_from_slice(&encoded[..frame.uncompressed_size as usize]);
        }

        pool.recycle(encoded);

        Ok(buffer)
    }

    pub async fn encode(
        &self,
        pool: &mut BufferPool,
        keys: &mut KeyManager,
        id: FileId,
        plain: &Buffer,
        encoding: Encoding,
    ) -> Result<Buffer, Error> {
        let mut encoded = pool.get(plain.len() / 2);

        if encoding.compression > 0 {
            compress_into(encoding.compression as i32, plain, &mut encoded)?;
        } else {
            encoded.extend_from_slice(plain);
        }

        if encoding.encrypted {
            let key = keys.get_encryption_key(id).await;
            encrypt_in_place(key, &mut encoded)?;
        }

        Ok(encoded)
    }
}
