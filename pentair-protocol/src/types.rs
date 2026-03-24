use crate::codec::Cursor;
use crate::error::Result;

/// Calculate the number of padding bytes needed to align `len` to a 4-byte boundary.
fn padding_for(len: usize) -> usize {
    let remainder = len % 4;
    if remainder == 0 {
        0
    } else {
        4 - remainder
    }
}

// ── SLString ───────────────────────────────────────────────────────────

/// Decode an SLString from the cursor.
///
/// Wire format: u32le length prefix, followed by `length` bytes of string data,
/// then zero-padded to the next 4-byte boundary.
pub fn decode_sl_string(cursor: &mut Cursor) -> Result<String> {
    let len = cursor.read_u32le()? as usize;
    let bytes = cursor.read_bytes(len)?;
    let s = String::from_utf8_lossy(bytes).into_owned();
    let pad = padding_for(len);
    if pad > 0 {
        cursor.skip(pad)?;
    }
    Ok(s)
}

/// Encode a string as an SLString.
///
/// Returns: u32le length prefix + string bytes + zero padding to 4-byte alignment.
pub fn encode_sl_string(s: &str) -> Vec<u8> {
    let len = s.len() as u32;
    let pad = padding_for(s.len());
    let total = 4 + s.len() + pad;
    let mut buf = Vec::with_capacity(total);
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(s.as_bytes());
    buf.extend(std::iter::repeat(0u8).take(pad));
    buf
}

// ── SLArray ────────────────────────────────────────────────────────────

/// Decode an SLArray from the cursor.
///
/// Wire format: u32le length prefix, followed by `length` raw bytes,
/// then zero-padded to the next 4-byte boundary.
pub fn decode_sl_array(cursor: &mut Cursor) -> Result<Vec<u8>> {
    let len = cursor.read_u32le()? as usize;
    let bytes = cursor.read_bytes(len)?;
    let result = bytes.to_vec();
    let pad = padding_for(len);
    if pad > 0 {
        cursor.skip(pad)?;
    }
    Ok(result)
}

/// Encode raw bytes as an SLArray.
///
/// Returns: u32le length prefix + data bytes + zero padding to 4-byte alignment.
pub fn encode_sl_array(data: &[u8]) -> Vec<u8> {
    let len = data.len() as u32;
    let pad = padding_for(data.len());
    let total = 4 + data.len() + pad;
    let mut buf = Vec::with_capacity(total);
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(data);
    buf.extend(std::iter::repeat(0u8).take(pad));
    buf
}

// ── SLDateTime ─────────────────────────────────────────────────────────

/// Controller date/time — fixed 16 bytes on the wire, no length prefix.
///
/// Layout: 8 consecutive u16 LE fields.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SLDateTime {
    pub year: u16,
    pub month: u16,
    pub day_of_week: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
    pub millisecond: u16,
}

/// Decode an SLDateTime (16 bytes) from the cursor.
pub fn decode_sl_datetime(cursor: &mut Cursor) -> Result<SLDateTime> {
    Ok(SLDateTime {
        year: cursor.read_u16le()?,
        month: cursor.read_u16le()?,
        day_of_week: cursor.read_u16le()?,
        day: cursor.read_u16le()?,
        hour: cursor.read_u16le()?,
        minute: cursor.read_u16le()?,
        second: cursor.read_u16le()?,
        millisecond: cursor.read_u16le()?,
    })
}

/// Encode an SLDateTime into 16 bytes (8 × u16 LE).
pub fn encode_sl_datetime(dt: &SLDateTime) -> Vec<u8> {
    let mut buf = Vec::with_capacity(16);
    buf.extend_from_slice(&dt.year.to_le_bytes());
    buf.extend_from_slice(&dt.month.to_le_bytes());
    buf.extend_from_slice(&dt.day_of_week.to_le_bytes());
    buf.extend_from_slice(&dt.day.to_le_bytes());
    buf.extend_from_slice(&dt.hour.to_le_bytes());
    buf.extend_from_slice(&dt.minute.to_le_bytes());
    buf.extend_from_slice(&dt.second.to_le_bytes());
    buf.extend_from_slice(&dt.millisecond.to_le_bytes());
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{decode_header, HEADER_SIZE};

    // ── SLString unit tests ────────────────────────────────────────────

    #[test]
    fn sl_string_roundtrip_aligned() {
        // "ABCD" is 4 bytes — already aligned, no padding needed
        let encoded = encode_sl_string("ABCD");
        assert_eq!(encoded.len(), 8); // 4 length + 4 chars
        let mut cursor = Cursor::new(&encoded);
        let decoded = decode_sl_string(&mut cursor).unwrap();
        assert_eq!(decoded, "ABCD");
        assert_eq!(cursor.remaining(), 0);
    }

    #[test]
    fn sl_string_roundtrip_unaligned() {
        // "ABC" is 3 bytes — needs 1 byte of padding
        let encoded = encode_sl_string("ABC");
        assert_eq!(encoded.len(), 8); // 4 length + 3 chars + 1 pad
                                      // Verify the padding byte is zero
        assert_eq!(encoded[7], 0);
        let mut cursor = Cursor::new(&encoded);
        let decoded = decode_sl_string(&mut cursor).unwrap();
        assert_eq!(decoded, "ABC");
        assert_eq!(cursor.remaining(), 0);
    }

    #[test]
    fn sl_string_empty() {
        let encoded = encode_sl_string("");
        assert_eq!(encoded.len(), 4); // just the length prefix (0), no padding needed
        let mut cursor = Cursor::new(&encoded);
        let decoded = decode_sl_string(&mut cursor).unwrap();
        assert_eq!(decoded, "");
    }

    #[test]
    fn sl_string_one_byte() {
        let encoded = encode_sl_string("X");
        // 1 byte string needs 3 bytes of padding → 4 + 1 + 3 = 8
        assert_eq!(encoded.len(), 8);
        let mut cursor = Cursor::new(&encoded);
        let decoded = decode_sl_string(&mut cursor).unwrap();
        assert_eq!(decoded, "X");
    }

    #[test]
    fn sl_string_five_bytes() {
        let encoded = encode_sl_string("Hello");
        // 5 bytes string needs 3 bytes of padding → 4 + 5 + 3 = 12
        assert_eq!(encoded.len(), 12);
        let mut cursor = Cursor::new(&encoded);
        let decoded = decode_sl_string(&mut cursor).unwrap();
        assert_eq!(decoded, "Hello");
    }

    #[test]
    fn sl_string_buffer_too_short() {
        let data = [0x05, 0x00, 0x00, 0x00, 0x41, 0x42]; // claims 5 bytes, only has 2
        let mut cursor = Cursor::new(&data);
        assert!(decode_sl_string(&mut cursor).is_err());
    }

    // ── SLArray unit tests ─────────────────────────────────────────────

    #[test]
    fn sl_array_roundtrip_aligned() {
        let data = [0x01, 0x02, 0x03, 0x04];
        let encoded = encode_sl_array(&data);
        assert_eq!(encoded.len(), 8); // 4 prefix + 4 data
        let mut cursor = Cursor::new(&encoded);
        let decoded = decode_sl_array(&mut cursor).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn sl_array_roundtrip_unaligned() {
        let data = [0x01, 0x02, 0x03];
        let encoded = encode_sl_array(&data);
        assert_eq!(encoded.len(), 8); // 4 prefix + 3 data + 1 pad
        let mut cursor = Cursor::new(&encoded);
        let decoded = decode_sl_array(&mut cursor).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn sl_array_empty() {
        let encoded = encode_sl_array(&[]);
        assert_eq!(encoded.len(), 4);
        let mut cursor = Cursor::new(&encoded);
        let decoded = decode_sl_array(&mut cursor).unwrap();
        assert!(decoded.is_empty());
    }

    #[test]
    fn sl_array_16_bytes() {
        let data = [0u8; 16];
        let encoded = encode_sl_array(&data);
        assert_eq!(encoded.len(), 20); // 4 prefix + 16 data (already aligned)
        let mut cursor = Cursor::new(&encoded);
        let decoded = decode_sl_array(&mut cursor).unwrap();
        assert_eq!(decoded, data);
    }

    // ── SLDateTime unit tests ──────────────────────────────────────────

    #[test]
    fn sl_datetime_roundtrip() {
        let dt = SLDateTime {
            year: 2026,
            month: 3,
            day_of_week: 1,
            day: 17,
            hour: 23,
            minute: 38,
            second: 45,
            millisecond: 0,
        };
        let encoded = encode_sl_datetime(&dt);
        assert_eq!(encoded.len(), 16);
        let mut cursor = Cursor::new(&encoded);
        let decoded = decode_sl_datetime(&mut cursor).unwrap();
        assert_eq!(decoded, dt);
    }

    #[test]
    fn sl_datetime_buffer_too_short() {
        let data = [0u8; 14]; // need 16
        let mut cursor = Cursor::new(&data);
        assert!(decode_sl_datetime(&mut cursor).is_err());
    }

    // ── Fixture-based tests ────────────────────────────────────────────

    #[test]
    fn fixture_version_response_string() {
        let data = include_bytes!("../../test-fixtures/version_response.bin");
        let header = decode_header(data).unwrap();
        assert_eq!(header.action, 8121);

        let payload = &data[HEADER_SIZE..];
        let mut cursor = Cursor::new(payload);
        let version = decode_sl_string(&mut cursor).unwrap();
        assert_eq!(version, "POOL: 5.2 Build 738.0 Rel");
    }

    #[test]
    fn fixture_system_time_response() {
        let data = include_bytes!("../../test-fixtures/system_time_response.bin");
        let header = decode_header(data).unwrap();
        assert_eq!(header.action, 8111);
        assert_eq!(header.data_length, 20);

        let payload = &data[HEADER_SIZE..];
        let mut cursor = Cursor::new(payload);

        let dt = decode_sl_datetime(&mut cursor).unwrap();
        assert_eq!(dt.year, 2026);
        assert_eq!(dt.month, 3);
        assert_eq!(dt.day_of_week, 1); // Monday
        assert_eq!(dt.day, 17);
        assert_eq!(dt.hour, 23);
        assert_eq!(dt.minute, 38);
        assert_eq!(dt.second, 45);
        assert_eq!(dt.millisecond, 0);

        let adjust_for_dst = cursor.read_i32le().unwrap();
        assert_eq!(adjust_for_dst, 1);
        assert_eq!(cursor.remaining(), 0);
    }

    #[test]
    fn fixture_login_request() {
        let data = include_bytes!("../../test-fixtures/login_request.bin");
        let header = decode_header(data).unwrap();
        assert_eq!(header.action, 27);
        assert_eq!(header.data_length, 44);

        let payload = &data[HEADER_SIZE..];
        let mut cursor = Cursor::new(payload);

        // schema
        let schema = cursor.read_i32le().unwrap();
        assert_eq!(schema, 348);

        // connection_type
        let connection_type = cursor.read_i32le().unwrap();
        assert_eq!(connection_type, 0);

        // client_version (SLString "Android")
        let client_version = decode_sl_string(&mut cursor).unwrap();
        assert_eq!(client_version, "Android");

        // password (SLArray of 16 zero bytes)
        let password = decode_sl_array(&mut cursor).unwrap();
        assert_eq!(password.len(), 16);
        assert!(password.iter().all(|&b| b == 0));

        // process_id
        let process_id = cursor.read_i32le().unwrap();
        assert_eq!(process_id, 2);

        assert_eq!(cursor.remaining(), 0);
    }
}
