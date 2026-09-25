//! Chrome Native Messaging framing: each JSON message is prefixed with its
//! length as a 32-bit integer in native byte order (spec §35).

use std::io::{Read, Write};

use crate::protocol::MAX_MESSAGE_BYTES;

#[derive(Debug, thiserror::Error)]
pub enum FramingError {
    #[error("stream closed")]
    Closed,
    #[error("message length {0} exceeds the maximum accepted size")]
    TooLarge(u32),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Read one length-prefixed message.
pub fn read_message<R: Read>(reader: &mut R) -> Result<Vec<u8>, FramingError> {
    let mut length_bytes = [0u8; 4];
    match reader.read_exact(&mut length_bytes) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => {
            return Err(FramingError::Closed)
        }
        Err(err) => return Err(FramingError::Io(err)),
    }

    let length = u32::from_ne_bytes(length_bytes);
    if length as usize > MAX_MESSAGE_BYTES {
        return Err(FramingError::TooLarge(length));
    }

    let mut buffer = vec![0u8; length as usize];
    reader.read_exact(&mut buffer)?;
    Ok(buffer)
}

/// Write one length-prefixed message.
pub fn write_message<W: Write>(writer: &mut W, body: &[u8]) -> Result<(), FramingError> {
    if body.len() > MAX_MESSAGE_BYTES {
        return Err(FramingError::TooLarge(body.len() as u32));
    }
    writer.write_all(&(body.len() as u32).to_ne_bytes())?;
    writer.write_all(body)?;
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_message() {
        let mut buffer: Vec<u8> = Vec::new();
        write_message(&mut buffer, br#"{"hello":true}"#).unwrap();
        let mut cursor = std::io::Cursor::new(buffer);
        assert_eq!(read_message(&mut cursor).unwrap(), br#"{"hello":true}"#);
        assert!(matches!(
            read_message(&mut cursor),
            Err(FramingError::Closed)
        ));
    }

    #[test]
    fn refuses_absurd_lengths() {
        let mut buffer = (u32::MAX).to_ne_bytes().to_vec();
        buffer.extend_from_slice(b"junk");
        let mut cursor = std::io::Cursor::new(buffer);
        assert!(matches!(
            read_message(&mut cursor),
            Err(FramingError::TooLarge(_))
        ));
    }
}
