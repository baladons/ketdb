use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{AeadInOut, Generate, KeyInit},
};

use crate::{context::Context, footer::BlockEncoding};

pub struct BlockEncoder {
    buffer: Vec<u8>,
}

impl BlockEncoder {
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    pub fn encode<'a>(&'a mut self, data: &[u8], encoding: &BlockEncoding) -> &'a [u8] {
        self.buffer.clear();

        if encoding.compression != 0 {
            compress_into(encoding.compression as i32, data, &mut self.buffer);
        } else {
            self.buffer.extend_from_slice(data);
        }

        if encoding.key_id != 0 {
            let key = Context::current()
                .key_vault
                .get(encoding.key_id)
                .expect("missing key for encrypted block");
            encrypt(key, &mut self.buffer);
        }

        &self.buffer
    }
}

pub struct BlockDecoder {
    buffer: Vec<u8>,
    scratch: Vec<u8>,
}

impl BlockDecoder {
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            scratch: Vec::new(),
        }
    }

    pub fn decode<'a>(
        &'a mut self,
        data: &[u8],
        encoding: &BlockEncoding,
        uncompressed_size: usize,
    ) -> &'a [u8] {
        self.buffer.clear();
        self.buffer.extend_from_slice(data);

        if encoding.key_id != 0 {
            let key = Context::current()
                .key_vault
                .get(encoding.key_id)
                .expect("missing key for encrypted block");
            decrypt(key, &mut self.buffer);
        }

        if encoding.compression != 0 {
            self.scratch.clear();
            self.scratch.reserve(uncompressed_size);
            decompress_into(&self.buffer, &mut self.scratch);
            std::mem::swap(&mut self.buffer, &mut self.scratch);
        }

        &self.buffer
    }
}

pub fn compress_into(level: i32, data: &[u8], buffer: &mut Vec<u8>) {
    zstd::stream::copy_encode(&mut &data[..], buffer, level).unwrap();
}

pub fn decompress_into(data: &[u8], buffer: &mut Vec<u8>) {
    zstd::bulk::decompress_to_buffer(data, buffer).unwrap();
}

pub fn encrypt(key: &[u8], data: &mut Vec<u8>) {
    let cipher = Aes256Gcm::new_from_slice(key).unwrap();
    let nonce = Nonce::generate();

    cipher.encrypt_in_place(&nonce, b"", data).unwrap();
    data.extend_from_slice(nonce.as_slice());
}

pub fn decrypt(key: &[u8], data: &mut Vec<u8>) {
    let cipher = Aes256Gcm::new_from_slice(key).unwrap();
    let nonce_bytes = data.split_off(data.len() - 12);
    let nonce_bytes: [u8; 12] = nonce_bytes.try_into().unwrap();
    let nonce = Nonce::from(nonce_bytes);
    cipher.decrypt_in_place(&nonce, b"", data).unwrap();
}
