/// Prefix carried by every minted personal access token.
pub const PERSONAL_TOKEN_PREFIX: &str = "pnpr_";

const PAYLOAD_BASE62_LEN: usize = 43;
const CRC32_BASE62_LEN: usize = 6;

/// Character length of a minted `pnpr_` personal access token (`5 + 43 + 6`).
pub const PERSONAL_TOKEN_LEN: usize =
    PERSONAL_TOKEN_PREFIX.len() + PAYLOAD_BASE62_LEN + CRC32_BASE62_LEN;

const BASE62_ALPHABET: &[u8; 62] =
    b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

pub(crate) fn mint_token() -> String {
    format_personal_token(super::fresh_secret())
}

pub(crate) fn format_personal_token(entropy: [u8; 32]) -> String {
    let payload = encode_base62_256(entropy);
    let checksum = encode_base62_u32(crc32_ieee(&payload));
    let mut out = String::with_capacity(PERSONAL_TOKEN_LEN);
    out.push_str(PERSONAL_TOKEN_PREFIX);
    out.push_str(std::str::from_utf8(&payload).expect("base62 alphabet is ASCII"));
    out.push_str(std::str::from_utf8(&checksum).expect("base62 alphabet is ASCII"));
    out
}

/// Whether `raw` has the `pnpr_` personal token shape and a matching IEEE
/// CRC-32 checksum over its 43-character base62 entropy payload.
#[must_use]
pub fn has_valid_personal_token_checksum(raw: &str) -> bool {
    let Some(rest) = raw.strip_prefix(PERSONAL_TOKEN_PREFIX) else {
        return false;
    };
    if rest.len() != PAYLOAD_BASE62_LEN + CRC32_BASE62_LEN {
        return false;
    }
    let (payload, checksum) = rest.as_bytes().split_at(PAYLOAD_BASE62_LEN);
    if !payload
        .iter()
        .copied()
        .all(|byte| decode_base62_digit(byte).is_some())
    {
        return false;
    }
    let Some(expected_crc) = decode_base62_u32(checksum) else {
        return false;
    };
    crc32_ieee(payload) == expected_crc
}

fn encode_base62_256(mut bytes: [u8; 32]) -> [u8; PAYLOAD_BASE62_LEN] {
    let mut out = [b'0'; PAYLOAD_BASE62_LEN];
    for slot in out.iter_mut().rev() {
        let mut rem = 0u16;
        for byte in &mut bytes {
            let acc = (rem << 8) | u16::from(*byte);
            *byte = (acc / 62) as u8;
            rem = acc % 62;
        }
        *slot = BASE62_ALPHABET[usize::from(rem)];
    }
    out
}

fn encode_base62_u32(mut value: u32) -> [u8; CRC32_BASE62_LEN] {
    let mut out = [b'0'; CRC32_BASE62_LEN];
    for slot in out.iter_mut().rev() {
        *slot = BASE62_ALPHABET[(value % 62) as usize];
        value /= 62;
    }
    out
}

fn decode_base62_u32(digits: &[u8]) -> Option<u32> {
    let mut acc = 0u64;
    for &byte in digits {
        let digit = u64::from(decode_base62_digit(byte)?);
        acc = acc.checked_mul(62)?.checked_add(digit)?;
    }
    u32::try_from(acc).ok()
}

const fn decode_base62_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'A'..=b'Z' => Some(byte - b'A' + 10),
        b'a'..=b'z' => Some(byte - b'a' + 36),
        _ => None,
    }
}

pub(crate) fn crc32_ieee(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}
