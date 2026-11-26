//! Nix's base32: the digits `0-9a-z` without `e`, `o`, `u`, and `t`, with the
//! least significant bits of the input in the last character.

use derive_more::{
    Display,
    Error,
};

const ALPHABET: &[u8; 32] = b"0123456789abcdfghijklmnpqrsvwxyz";

/// A string that is not the Nix base32 encoding of the expected byte count.
#[derive(Debug, Display, Error, Clone, Copy, PartialEq, Eq)]
#[display("not a Nix base32 string of the expected length")]
pub struct DecodeError;

/// The length of the encoding of `byte_count` bytes.
pub const fn encoded_len(byte_count: usize) -> usize {
    (byte_count * 8).div_ceil(5)
}

pub fn encode(bytes: &[u8]) -> String {
    (0..encoded_len(bytes.len()))
        .rev()
        .map(|position| {
            let bit = position * 5;
            let (index, shift) = (bit / 8, bit % 8);
            let low = u16::from(bytes[index]) >> shift;
            let high = bytes
                .get(index + 1)
                .map_or(0, |next| u16::from(*next) << (8 - shift));
            char::from(ALPHABET[usize::from((low | high) & 0x1f)])
        })
        .collect()
}

/// Decodes `text` into exactly `byte_count` bytes.
pub fn decode(text: &str, byte_count: usize) -> Result<Vec<u8>, DecodeError> {
    if text.len() != encoded_len(byte_count) {
        return Err(DecodeError);
    }
    let mut bytes = vec![0_u8; byte_count];
    for (position, character) in text.bytes().rev().enumerate() {
        let digit = digit_value(character).ok_or(DecodeError)?;
        let bit = position * 5;
        let (index, shift) = (bit / 8, bit % 8);
        let spread = u16::from(digit) << shift;
        bytes[index] |= spread.to_le_bytes()[0];
        let carry = spread.to_le_bytes()[1];
        match bytes.get_mut(index + 1) {
            Some(next) => *next |= carry,
            None if carry != 0 => return Err(DecodeError),
            None => {}
        }
    }
    Ok(bytes)
}

fn digit_value(character: u8) -> Option<u8> {
    ALPHABET
        .iter()
        .position(|candidate| *candidate == character)
        .and_then(|index| u8::try_from(index).ok())
}

/// Whether every byte of `text` is a Nix base32 digit.
pub fn is_valid(text: &str) -> bool {
    text.bytes()
        .all(|character| digit_value(character).is_some())
}

#[cfg(test)]
mod tests;
