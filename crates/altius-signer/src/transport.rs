//! Length-prefixed JSON framing shared by the client and server sides of
//! the Unix-domain-socket IPC. Not a public API — both sides just need to
//! agree on the same framing.

use std::io::{Read, Write};

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::error::SignerError;

pub(crate) fn write_message<W: Write, T: Serialize>(
    writer: &mut W,
    value: &T,
) -> Result<(), SignerError> {
    let payload = serde_json::to_vec(value)?;
    let len =
        u32::try_from(payload.len()).map_err(|_| SignerError::MessageTooLarge(payload.len()))?;
    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(&payload)?;
    writer.flush()?;
    Ok(())
}

pub(crate) fn read_message<R: Read, T: DeserializeOwned>(reader: &mut R) -> Result<T, SignerError> {
    let mut len_bytes = [0u8; 4];
    match reader.read_exact(&mut len_bytes) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
            return Err(SignerError::ConnectionClosed)
        }
        Err(e) => return Err(e.into()),
    }
    let len = u32::from_le_bytes(len_bytes) as usize;
    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload)?;
    Ok(serde_json::from_slice(&payload)?)
}
