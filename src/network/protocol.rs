use crate::error::StrangecoinError;
use std::io::Read;

pub const MAX_MESSAGE_SIZE: usize = 32 * 1024 * 1024;
pub const MAX_BLOCK_SIZE: usize = 4 * 1024 * 1024;
pub const MAX_TX_SIZE: usize = 256 * 1024;

pub fn read_length_prefixed<R: Read>(reader: &mut R) -> Result<Vec<u8>, StrangecoinError> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).map_err(|_| {
        StrangecoinError::IoError(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "length prefix",
        ))
    })?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_MESSAGE_SIZE {
        return Err(StrangecoinError::SizeLimitExceeded("message"));
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).map_err(|_| {
        StrangecoinError::IoError(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "message body",
        ))
    })?;
    Ok(buf)
}
