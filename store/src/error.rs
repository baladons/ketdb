#[derive(Debug)]
pub enum Error {
    SerializationFailure,
    DeserializationFailure,
    InvalidKey,
    EncryptionFailure,
    DecryptionFailure,
    CompressionFailure,
    DecompressionFailure,
    IoError(std::io::Error),
    CrcMismatch,
}
