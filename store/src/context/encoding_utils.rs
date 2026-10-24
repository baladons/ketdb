use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{AeadInOut, Generate, KeyInit},
};

use crate::{buffer::Buffer, error::Error};

pub fn encrypt_in_place(key: &[u8], buffer: &mut Buffer) -> Result<(), Error> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| Error::InvalidKey)?;
    let nonce = Nonce::generate();

    cipher
        .encrypt_in_place(&nonce, b"", buffer)
        .map_err(|_| Error::EncryptionFailure)?;

    buffer.extend_from_slice(nonce.as_slice());

    Ok(())
}

pub fn decrypt_in_place(key: &[u8], buffer: &mut Buffer) -> Result<(), Error> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| Error::InvalidKey)?;
    let nonce_bytes = &buffer[buffer.len() - 12..];
    let nonce_bytes: [u8; 12] = nonce_bytes
        .try_into()
        .map_err(|_| Error::DecryptionFailure)?;
    let nonce = Nonce::from(nonce_bytes);
    cipher
        .decrypt_in_place(&nonce, b"", buffer)
        .map_err(|_| Error::DecryptionFailure)?;

    Ok(())
}

pub fn compress_into(level: i32, raw: &[u8], compressed: &mut Buffer) -> Result<(), Error> {
    zstd::stream::copy_encode(&mut &raw[..], compressed.inner_mut(), level)
        .map_err(|_| Error::CompressionFailure)?;

    Ok(())
}

pub fn decompress_into(compressed: &[u8], decompressed: &mut Buffer) -> Result<(), Error> {
    zstd::bulk::decompress_to_buffer(compressed, decompressed)
        .map_err(|_| Error::DecompressionFailure)?;

    Ok(())
}
