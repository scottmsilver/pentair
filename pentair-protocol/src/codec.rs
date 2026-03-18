use crate::error::{ProtocolError, Result};

/// Message header size in bytes: header_id(u16le) + action(u16le) + data_length(u32le).
pub const HEADER_SIZE: usize = 8;

/// Decoded message header.
#[derive(Debug, Clone, PartialEq)]
pub struct MessageHeader {
    pub header_id: u16,
    pub action: u16,
    pub data_length: u32,
}

/// A zero-copy cursor over a byte slice for reading protocol primitives.
pub struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    /// Create a new cursor positioned at the start of the given data.
    pub fn new(data: &'a [u8]) -> Self {
        Cursor { data, pos: 0 }
    }

    /// Returns the number of bytes remaining from the current position.
    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    /// Returns the current byte position within the buffer.
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Read a single byte, advancing the cursor by 1.
    pub fn read_u8(&mut self) -> Result<u8> {
        if self.remaining() < 1 {
            return Err(ProtocolError::BufferTooShort {
                need: 1,
                have: self.remaining(),
            });
        }
        let val = self.data[self.pos];
        self.pos += 1;
        Ok(val)
    }

    /// Read a u16 in little-endian byte order.
    pub fn read_u16le(&mut self) -> Result<u16> {
        let bytes = self.read_bytes(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    /// Read a u16 in big-endian byte order.
    pub fn read_u16be(&mut self) -> Result<u16> {
        let bytes = self.read_bytes(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    /// Read an i32 in little-endian byte order.
    pub fn read_i32le(&mut self) -> Result<i32> {
        let bytes = self.read_bytes(4)?;
        Ok(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Read a u32 in little-endian byte order.
    pub fn read_u32le(&mut self) -> Result<u32> {
        let bytes = self.read_bytes(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Read a u32 in big-endian byte order.
    pub fn read_u32be(&mut self) -> Result<u32> {
        let bytes = self.read_bytes(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Read exactly `n` bytes as a borrowed slice, advancing the cursor.
    pub fn read_bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.remaining() < n {
            return Err(ProtocolError::BufferTooShort {
                need: n,
                have: self.remaining(),
            });
        }
        let slice = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }

    /// Skip `n` bytes, advancing the cursor without returning data.
    pub fn skip(&mut self, n: usize) -> Result<()> {
        if self.remaining() < n {
            return Err(ProtocolError::BufferTooShort {
                need: n,
                have: self.remaining(),
            });
        }
        self.pos += n;
        Ok(())
    }
}

/// Decode an 8-byte message header from the given data.
pub fn decode_header(data: &[u8]) -> Result<MessageHeader> {
    let mut cursor = Cursor::new(data);
    let header_id = cursor.read_u16le()?;
    let action = cursor.read_u16le()?;
    let data_length = cursor.read_u32le()?;
    Ok(MessageHeader {
        header_id,
        action,
        data_length,
    })
}

/// Encode a message with the given action code and payload.
///
/// The header_id is always set to 0 for client-originated messages.
/// Returns the complete message bytes (8-byte header + payload).
pub fn encode_message(action: u16, payload: &[u8]) -> Vec<u8> {
    let data_length = payload.len() as u32;
    let mut buf = Vec::with_capacity(HEADER_SIZE + payload.len());
    buf.extend_from_slice(&0u16.to_le_bytes()); // header_id = 0
    buf.extend_from_slice(&action.to_le_bytes());
    buf.extend_from_slice(&data_length.to_le_bytes());
    buf.extend_from_slice(payload);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Cursor unit tests ──────────────────────────────────────────────

    #[test]
    fn cursor_new_empty() {
        let c = Cursor::new(&[]);
        assert_eq!(c.remaining(), 0);
        assert_eq!(c.position(), 0);
    }

    #[test]
    fn cursor_read_u8() {
        let data = [0xAB];
        let mut c = Cursor::new(&data);
        assert_eq!(c.read_u8().unwrap(), 0xAB);
        assert_eq!(c.remaining(), 0);
    }

    #[test]
    fn cursor_read_u8_buffer_too_short() {
        let mut c = Cursor::new(&[]);
        assert!(c.read_u8().is_err());
    }

    #[test]
    fn cursor_read_u16le() {
        let data = [0x01, 0x02]; // LE: 0x0201 = 513
        let mut c = Cursor::new(&data);
        assert_eq!(c.read_u16le().unwrap(), 0x0201);
    }

    #[test]
    fn cursor_read_u16be() {
        let data = [0x01, 0x02]; // BE: 0x0102 = 258
        let mut c = Cursor::new(&data);
        assert_eq!(c.read_u16be().unwrap(), 0x0102);
    }

    #[test]
    fn cursor_read_i32le() {
        // -1 in i32 LE = [0xFF, 0xFF, 0xFF, 0xFF]
        let data = [0xFF, 0xFF, 0xFF, 0xFF];
        let mut c = Cursor::new(&data);
        assert_eq!(c.read_i32le().unwrap(), -1);
    }

    #[test]
    fn cursor_read_u32le() {
        let data = [0x38, 0x00, 0x00, 0x00]; // 56
        let mut c = Cursor::new(&data);
        assert_eq!(c.read_u32le().unwrap(), 56);
    }

    #[test]
    fn cursor_read_u32be() {
        let data = [0x00, 0x00, 0x00, 0x38]; // 56
        let mut c = Cursor::new(&data);
        assert_eq!(c.read_u32be().unwrap(), 56);
    }

    #[test]
    fn cursor_read_bytes() {
        let data = [1, 2, 3, 4, 5];
        let mut c = Cursor::new(&data);
        let slice = c.read_bytes(3).unwrap();
        assert_eq!(slice, &[1, 2, 3]);
        assert_eq!(c.remaining(), 2);
        assert_eq!(c.position(), 3);
    }

    #[test]
    fn cursor_read_bytes_buffer_too_short() {
        let data = [1, 2];
        let mut c = Cursor::new(&data);
        assert!(c.read_bytes(3).is_err());
    }

    #[test]
    fn cursor_skip() {
        let data = [1, 2, 3, 4, 5];
        let mut c = Cursor::new(&data);
        c.skip(3).unwrap();
        assert_eq!(c.position(), 3);
        assert_eq!(c.read_u8().unwrap(), 4);
    }

    #[test]
    fn cursor_skip_buffer_too_short() {
        let data = [1, 2];
        let mut c = Cursor::new(&data);
        assert!(c.skip(3).is_err());
    }

    // ── Header / message framing tests ─────────────────────────────────

    #[test]
    fn header_roundtrip() {
        let payload = [0xDE, 0xAD, 0xBE, 0xEF];
        let action = 8121u16;
        let msg = encode_message(action, &payload);

        assert_eq!(msg.len(), HEADER_SIZE + payload.len());

        let header = decode_header(&msg).unwrap();
        assert_eq!(header.header_id, 0);
        assert_eq!(header.action, action);
        assert_eq!(header.data_length, payload.len() as u32);
    }

    #[test]
    fn header_empty_payload() {
        let msg = encode_message(16, &[]);
        let header = decode_header(&msg).unwrap();
        assert_eq!(header.action, 16);
        assert_eq!(header.data_length, 0);
    }

    #[test]
    fn decode_header_buffer_too_short() {
        let data = [0x00, 0x00, 0x1B]; // only 3 bytes
        assert!(decode_header(&data).is_err());
    }

    // ── Fixture-based tests ────────────────────────────────────────────

    #[test]
    fn fixture_version_response_header() {
        let data = include_bytes!("../../test-fixtures/version_response.bin");
        assert_eq!(data.len(), 64);

        let header = decode_header(data).unwrap();
        assert_eq!(header.header_id, 0);
        assert_eq!(header.action, 8121);
        assert_eq!(header.data_length, 56);
    }

    #[test]
    fn fixture_system_time_response_header() {
        let data = include_bytes!("../../test-fixtures/system_time_response.bin");
        assert_eq!(data.len(), 28);

        let header = decode_header(data).unwrap();
        assert_eq!(header.header_id, 0);
        assert_eq!(header.action, 8111);
        assert_eq!(header.data_length, 20);
    }

    #[test]
    fn fixture_login_request_header() {
        let data = include_bytes!("../../test-fixtures/login_request.bin");
        assert_eq!(data.len(), 52);

        let header = decode_header(data).unwrap();
        assert_eq!(header.header_id, 0);
        assert_eq!(header.action, 27);
        assert_eq!(header.data_length, 44);
    }
}
