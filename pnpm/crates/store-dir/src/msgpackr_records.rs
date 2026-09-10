//! Encoder and decoder for the narrow subset of
//! [msgpackr](https://github.com/kriszyp/msgpackr)'s wire format that
//! pnpm v11 uses to write `index.db` rows — standard `MessagePack`
//! extended with msgpackr's **records** extension.
//!
//! ## Why this exists
//!
//! pnpm packs every `PackageFilesIndex` with `useRecords: true` and
//! `moreTypes: true`. `useRecords` replaces repeated string keys in
//! same-shape structs with a compact slot reference — roughly, Protobuf
//! field numbers
//! inline. Plain `rmp_serde` output round-trips through msgpackr badly
//! in *both* directions:
//!
//! - **Reading pnpm → pacquet**: standard `rmp_serde` has no idea what
//!   records bytes mean, so a row pnpm wrote would fail to decode and
//!   look like a cache miss, forcing a full re-download.
//! - **Reading pacquet → pnpm**: msgpackr with `useRecords: true`
//!   decodes every plain msgpack map (at any nesting level) as a JS
//!   `Map`, including the top-level `PackageFilesIndex`. pnpm's code
//!   then does `pkgIndex.files` (a property access on that `Map`),
//!   gets `undefined`, and crashes with `files is not iterable`.
//!
//! This module provides both halves — [`transcode_to_plain_msgpack`]
//! for the read side and [`encode_package_files_index`] for the write
//! side — so a shared `index.db` actually works.
//!
//! ## Wire format (the parts pnpm actually emits)
//!
//! **Record definition** — a struct-shape declaration:
//! ```text
//! d4 72 <slot>    fixext1, ext type 0x72 ('r'), 1-byte payload = slot id
//! <array>         msgpack array of N field-name strings
//! <value 0>       raw msgpack value for field 0       ──┐
//! <value 1>       raw msgpack value for field 1         │ first instance,
//! …                                                     │ inlined
//! <value N-1>     raw msgpack value for field N-1     ──┘
//! ```
//! The slot byte is from `0x40..=0x7f`. (These bytes are where `MessagePack`
//! would normally encode positive fixints 64–127; inside a records stream
//! those values are instead hoisted into `uint 8`, so the range is free.)
//!
//! **Record reference** — every subsequent instance of a slot:
//! ```text
//! <slot>          single byte in 0x40..=0x7f
//! <value 0> … <value N-1>
//! ```
//!
//! Everything else (maps, arrays, strings, ints, bools, nil, floats) is
//! vanilla `MessagePack`. Despite `moreTypes: true`, pnpm's payloads encode
//! JS `Map` objects as standard msgpack `fixmap`/`map16`/`map32` — no
//! ext-type wrapping. `checkedAt` timestamps are written as `float 64`
//! because JS numbers are doubles.
//!
//! ## Strategy
//!
//! **Read side** ([`transcode_to_plain_msgpack`]): rather than
//! deserialize `PackageFilesIndex` directly from msgpackr bytes, we
//! transcode to vanilla `MessagePack` (expanding each record instance
//! into a string-keyed map) and hand the result to `rmp_serde`.
//! Reusing the existing `Deserialize` derive keeps the decoder focused
//! on the wire-format transformation and nothing else.
//!
//! **Write side** ([`encode_package_files_index`]): a hand-written
//! emitter that allocates slots lazily per distinct *record shape*
//! for `PackageFilesIndex`, `CafsFileInfo`, and `SideEffectsDiff` —
//! `0x40` is reserved for the top-level `PackageFilesIndex`, and
//! inner slots in `0x41..=0x7f` are handed out in first-seen order,
//! so a single Rust type can consume more than one slot when its
//! optional-field presence varies within the same row. `HashMap`
//! fields (`files`, `sideEffects`, `added`) stay as plain msgpack
//! maps. That shape matches what msgpackr itself emits for a JS
//! object containing `Map` fields, so pnpm's reader round-trips the
//! bytes correctly.

pub use encoding::{EncodeError, encode_package_files_index};

use crate::{CafsFileInfo, PackageFilesIndex, SideEffectsDiff};
use derive_more::{Display, Error};
use miette::Diagnostic;
use serde_json::Value;
use smart_default::SmartDefault;
use std::{collections::HashMap, rc::Rc};

/// Extension type code msgpackr assigns to record-definition markers.
/// ASCII 'r'. See msgpackr's README under "Records Extension".
///
/// Exposed so callers can cheaply sniff whether a byte buffer was written
/// with `useRecords: true` — the fixext1 header `d4 72` is a reliable
/// opener for pnpm-written rows because the top-level struct is always
/// a record.
pub const RECORD_DEF_EXT_TYPE: u8 = 0x72;

/// Byte range that encodes a record-slot reference.
const SLOT_LO: u8 = 0x40;
const SLOT_HI: u8 = 0x7f;

/// Error type of [`transcode_to_plain_msgpack`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum DecodeError {
    #[display("Unexpected end of MessagePack buffer at offset {offset}")]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_MSGPACKR_RECORDS_UNEXPECTED_EOF))]
    UnexpectedEof { offset: usize },

    #[display(
        "Reference to unknown record slot 0x{slot:02x} at offset {offset} — \
         the definition was missing or appeared later than its use"
    )]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_MSGPACKR_RECORDS_UNKNOWN_SLOT))]
    UnknownSlot { slot: u8, offset: usize },

    #[display(
        "Record definition at offset {offset} has slot 0x{slot:02x}, which \
         is outside the valid reference range 0x40..=0x7f — any reference \
         written for this slot would be unreachable"
    )]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_MSGPACKR_RECORDS_SLOT_OUT_OF_RANGE))]
    SlotOutOfRange { slot: u8, offset: usize },

    #[display(
        "Expected a msgpack array header (fixarray, array16, or array32) \
         for a record-definition field-name list at offset {offset}, got \
         byte 0x{byte:02x}"
    )]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_MSGPACKR_RECORDS_EXPECTED_ARRAY_HEADER))]
    ExpectedArrayHeader { byte: u8, offset: usize },

    #[display(
        "Expected a msgpack string header (fixstr, str8, str16, or str32) \
         for a record-definition field name at offset {offset}, got byte \
         0x{byte:02x}"
    )]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_MSGPACKR_RECORDS_EXPECTED_STRING_HEADER))]
    ExpectedStringHeader { byte: u8, offset: usize },

    #[display(
        "Field name in a record definition at offset {offset} contains \
         invalid UTF-8"
    )]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_MSGPACKR_RECORDS_INVALID_FIELD_NAME_UTF8))]
    InvalidFieldNameUtf8 { offset: usize },

    #[display("Unsupported msgpack header byte 0x{byte:02x} at offset {offset}")]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_MSGPACKR_RECORDS_UNSUPPORTED))]
    Unsupported { byte: u8, offset: usize },

    #[display("{count} bytes left over after decoding the top-level value")]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_MSGPACKR_RECORDS_TRAILING_BYTES))]
    TrailingBytes { count: usize },
}

/// Expand msgpackr records into a pure-MessagePack byte stream that
/// `rmp_serde` can deserialize.
///
/// `bytes` may already be pure msgpack (e.g. pacquet-written rows). The
/// bytes `0x40..=0x7f` are ambiguous — in vanilla `MessagePack` they're
/// positive fixints 64–127; inside a msgpackr-records stream they're
/// record-slot references. We disambiguate by tracking whether a record
/// definition has been seen in the stream so far: until the first
/// `d4 72 <slot>` header, those bytes are treated as fixints and the
/// transcoder behaves as a pass-through (modulo float-to-int narrowing,
/// which is always applied so the output can be deserialized into
/// integer-typed Rust fields).
pub fn transcode_to_plain_msgpack(bytes: &[u8]) -> Result<Vec<u8>, DecodeError> {
    let mut state = TranscodeState::default();
    let mut reader = Reader::new(bytes);
    let mut writer = Vec::with_capacity(bytes.len() + bytes.len() / 4);
    transcode_value(&mut reader, &mut writer, &mut state)?;
    let leftover = reader.remaining();
    if leftover != 0 {
        return Err(DecodeError::TrailingBytes { count: leftover });
    }
    Ok(writer)
}

/// Parser context threaded through [`transcode_value`]. Records mode
/// starts off and flips on the first record definition — msgpackr
/// doesn't re-emit positive fixints in the slot-byte range once records
/// mode is on, so the flip is one-way for any real stream.
///
/// Slot schemas live under `Rc<[String]>` so reference-path decoding
/// can bump a refcount instead of deep-cloning the field-name vector
/// on every record instance.
#[derive(Default)]
struct TranscodeState {
    slots: HashMap<u8, Rc<[String]>>,
    records_mode: bool,
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Reader { bytes, pos: 0 }
    }
    fn remaining(&self) -> usize {
        self.bytes.len() - self.pos
    }
    fn peek(&self, offset: usize) -> Result<u8, DecodeError> {
        self.bytes
            .get(self.pos + offset)
            .copied()
            .ok_or(DecodeError::UnexpectedEof { offset: self.pos + offset })
    }
    fn read_u8(&mut self) -> Result<u8, DecodeError> {
        let byte = self.peek(0)?;
        self.pos += 1;
        Ok(byte)
    }
    fn read_bytes(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let end = self.pos.checked_add(n).ok_or(DecodeError::UnexpectedEof { offset: self.pos })?;
        if end > self.bytes.len() {
            return Err(DecodeError::UnexpectedEof { offset: end });
        }
        let slice = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(slice)
    }
    fn read_u16(&mut self) -> Result<u16, DecodeError> {
        let bytes = self.read_bytes(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }
    fn read_u32(&mut self) -> Result<u32, DecodeError> {
        let bytes = self.read_bytes(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }
}

/// Transcode one logical value (which may be a record instance — i.e. a
/// compound thing spanning a def + N raw values).
fn transcode_value(
    reader: &mut Reader<'_>,
    writer: &mut Vec<u8>,
    state: &mut TranscodeState,
) -> Result<(), DecodeError> {
    let start = reader.pos;
    let head = reader.peek(0)?;

    // Record reference — only valid after records mode has been entered;
    // in plain MessagePack the same bytes are positive fixints 64–127.
    if state.records_mode && (SLOT_LO..=SLOT_HI).contains(&head) {
        reader.read_u8()?;
        // `Rc::clone` is a refcount bump — the `Vec<String>` of field
        // names isn't duplicated. We clone instead of borrowing so the
        // recursive `transcode_value` call below can take `&mut state`.
        let fields = Rc::clone(
            state.slots.get(&head).ok_or(DecodeError::UnknownSlot { slot: head, offset: start })?,
        );
        return transcode_record(reader, writer, state, &fields);
    }

    // Record definition — fixext1 with ext type 0x72. Followed by the field-name
    // array, then the first instance inlined. Seeing this header flips the
    // stream into records mode from here on.
    if head == 0xd4 && reader.peek(1)? == RECORD_DEF_EXT_TYPE {
        let fields = read_record_def(reader, state)?;
        return transcode_record(reader, writer, state, &fields);
    }

    match head {
        0x80..=0x9f | 0xdc..=0xdf => transcode_container(reader, writer, state, head),
        0xca | 0xcb => transcode_float(reader, writer, head),
        _ => transcode_scalar(reader, writer, head),
    }
}

fn transcode_container(
    reader: &mut Reader<'_>,
    writer: &mut Vec<u8>,
    state: &mut TranscodeState,
    head: u8,
) -> Result<(), DecodeError> {
    let count = if matches!(head, 0x80..=0x9f) {
        writer.push(reader.read_u8()?);
        (head & 0x0f) as usize
    } else {
        let length_bytes = if matches!(head, 0xdc | 0xde) { 2 } else { 4 };
        let count = peek_payload_length(reader, length_bytes)?;
        writer.extend_from_slice(reader.read_bytes(1 + length_bytes)?);
        count
    };
    if matches!(head, 0x80..=0x8f | 0xde | 0xdf) {
        transcode_pairs(reader, writer, state, count)
    } else {
        transcode_array(reader, writer, state, count)
    }
}

fn transcode_scalar(
    reader: &mut Reader<'_>,
    writer: &mut Vec<u8>,
    head: u8,
) -> Result<(), DecodeError> {
    match head {
        0x00..=0x7f | 0xe0..=0xff | 0xc0 | 0xc2 | 0xc3 => copy_n(reader, writer, 1),
        0xa0..=0xbf => copy_n(reader, writer, 1 + (head & 0x1f) as usize),
        0xc4 | 0xd9 => copy_payload(reader, writer, 1, 1),
        0xc5 | 0xda => copy_payload(reader, writer, 2, 1),
        0xc6 | 0xdb => copy_payload(reader, writer, 4, 1),
        0xc7 => copy_payload(reader, writer, 1, 2),
        0xc8 => copy_payload(reader, writer, 2, 2),
        0xc9 => copy_payload(reader, writer, 4, 2),
        0xcc | 0xd0 => copy_n(reader, writer, 2),
        0xcd | 0xd1 => copy_n(reader, writer, 3),
        0xce | 0xd2 => copy_n(reader, writer, 5),
        0xcf | 0xd3 => copy_n(reader, writer, 9),
        0xd4 => copy_n(reader, writer, 3),
        0xd5 => copy_n(reader, writer, 4),
        0xd6 => copy_n(reader, writer, 6),
        0xd7 => copy_n(reader, writer, 10),
        0xd8 => copy_n(reader, writer, 18),
        other => Err(DecodeError::Unsupported { byte: other, offset: reader.pos }),
    }
}

fn peek_payload_length(reader: &Reader<'_>, length_bytes: usize) -> Result<usize, DecodeError> {
    match length_bytes {
        1 => Ok(reader.peek(1)? as usize),
        2 => Ok(u16::from_be_bytes([reader.peek(1)?, reader.peek(2)?]) as usize),
        4 => Ok(u32::from_be_bytes([
            reader.peek(1)?,
            reader.peek(2)?,
            reader.peek(3)?,
            reader.peek(4)?,
        ]) as usize),
        _ => unreachable!("MessagePack lengths use one, two, or four bytes"),
    }
}

fn copy_payload(
    reader: &mut Reader<'_>,
    writer: &mut Vec<u8>,
    length_bytes: usize,
    tag_bytes: usize,
) -> Result<(), DecodeError> {
    let count = peek_payload_length(reader, length_bytes)?;
    copy_n(reader, writer, tag_bytes + length_bytes + count)
}

/// msgpackr encodes large JS integers as floats, but `rmp_serde` requires
/// integer representations for `size` and `checked_at`. Narrow only exact,
/// non-negative integers; other floats retain their original encoding.
fn transcode_float(
    reader: &mut Reader<'_>,
    writer: &mut Vec<u8>,
    head: u8,
) -> Result<(), DecodeError> {
    reader.read_u8()?;
    if head == 0xca {
        let bits = reader.read_bytes(4)?;
        let value = f32::from_be_bytes([bits[0], bits[1], bits[2], bits[3]]);
        maybe_narrow_float_to_uint(writer, f64::from(value), head, bits);
    } else {
        let bits = reader.read_bytes(8)?;
        let value = f64::from_be_bytes([
            bits[0], bits[1], bits[2], bits[3], bits[4], bits[5], bits[6], bits[7],
        ]);
        maybe_narrow_float_to_uint(writer, value, head, bits);
    }
    Ok(())
}

/// Emit a record instance as a plain `MessagePack` map: the definition's field
/// names paired with the raw values that follow.
fn transcode_record(
    reader: &mut Reader<'_>,
    writer: &mut Vec<u8>,
    state: &mut TranscodeState,
    fields: &Rc<[String]>,
) -> Result<(), DecodeError> {
    write_map_header(writer, fields.len());
    for name in fields.iter() {
        write_str(writer, name);
        transcode_value(reader, writer, state)?;
    }
    Ok(())
}

/// Register the record definition at the reader's position and return its
/// field names.
fn read_record_def(
    reader: &mut Reader<'_>,
    state: &mut TranscodeState,
) -> Result<Rc<[String]>, DecodeError> {
    reader.read_u8()?; // 0xd4
    reader.read_u8()?; // 0x72
    let slot_offset = reader.pos;
    let slot = reader.read_u8()?;
    // msgpackr only ever emits slot bytes in 0x40..=0x7f — any value
    // outside that range is either malformed input or a payload we
    // don't understand. Reject rather than silently registering a
    // slot that nothing could ever reference.
    if !(SLOT_LO..=SLOT_HI).contains(&slot) {
        return Err(DecodeError::SlotOutOfRange { slot, offset: slot_offset });
    }
    let fields: Rc<[String]> = read_string_array(reader)?.into();
    state.slots.insert(slot, Rc::clone(&fields));
    state.records_mode = true;
    Ok(fields)
}

fn transcode_array(
    reader: &mut Reader<'_>,
    writer: &mut Vec<u8>,
    state: &mut TranscodeState,
    n: usize,
) -> Result<(), DecodeError> {
    for _ in 0..n {
        transcode_value(reader, writer, state)?;
    }
    Ok(())
}

fn transcode_pairs(
    reader: &mut Reader<'_>,
    writer: &mut Vec<u8>,
    state: &mut TranscodeState,
    n: usize,
) -> Result<(), DecodeError> {
    for _ in 0..n {
        transcode_value(reader, writer, state)?; // key
        transcode_value(reader, writer, state)?; // value
    }
    Ok(())
}

fn copy_n(reader: &mut Reader<'_>, writer: &mut Vec<u8>, n: usize) -> Result<(), DecodeError> {
    let bytes = reader.read_bytes(n)?;
    writer.extend_from_slice(bytes);
    Ok(())
}

/// Read a msgpack array of strings at the current reader position and
/// return its elements. Only fixarray + array16/32 are accepted — record
/// defs in the wild are always fixarray, but array16/32 costs nothing to
/// support and future-proofs against a pnpm release that widens schemas
/// past 15 fields.
fn read_string_array(reader: &mut Reader<'_>) -> Result<Vec<String>, DecodeError> {
    let start = reader.pos;
    let head = reader.read_u8()?;
    let len = match head {
        0x90..=0x9f => (head & 0x0f) as usize,
        0xdc => reader.read_u16()? as usize,
        0xdd => reader.read_u32()? as usize,
        _ => return Err(DecodeError::ExpectedArrayHeader { byte: head, offset: start }),
    };
    let mut out = Vec::with_capacity(len);
    for _ in 0..len {
        out.push(read_string(reader)?);
    }
    Ok(out)
}

fn read_string(reader: &mut Reader<'_>) -> Result<String, DecodeError> {
    let start = reader.pos;
    let head = reader.read_u8()?;
    let len = match head {
        0xa0..=0xbf => (head & 0x1f) as usize,
        0xd9 => reader.read_u8()? as usize,
        0xda => reader.read_u16()? as usize,
        0xdb => reader.read_u32()? as usize,
        _ => return Err(DecodeError::ExpectedStringHeader { byte: head, offset: start }),
    };
    let bytes = reader.read_bytes(len)?.to_vec();
    String::from_utf8(bytes).map_err(|_| DecodeError::InvalidFieldNameUtf8 { offset: start })
}

/// Exactly 2^64 as f64 — the smallest `f64` value that does **not** fit
/// in a `u64`. `u64::MAX as f64` rounds *up* to 2^64 (`u64::MAX` is
/// 2^64 − 1, which is not exactly representable in f64), so using it as
/// the inclusive upper bound would admit a literal 2^64 and silently
/// saturate to `u64::MAX` on cast.
const U64_MAX_EXCLUSIVE_AS_F64: f64 = 18_446_744_073_709_551_616.0;

/// If `v` is a finite non-negative integer value that strictly fits in
/// `u64`, emit it as msgpack `uint 64` (`cf` + 8 big-endian bytes).
/// Otherwise, pass through the original float header + payload
/// unchanged. The strict upper bound (`< 2^64`, not `<= u64::MAX as f64`)
/// prevents silent value corruption at the representable-but-overflowing
/// edge.
fn maybe_narrow_float_to_uint(
    writer: &mut Vec<u8>,
    value: f64,
    original_head: u8,
    original_bytes: &[u8],
) {
    if value.is_finite() && (0.0..U64_MAX_EXCLUSIVE_AS_F64).contains(&value) && value.fract() == 0.0
    {
        writer.push(0xcf);
        writer.extend_from_slice(&(value as u64).to_be_bytes());
    } else {
        writer.push(original_head);
        writer.extend_from_slice(original_bytes);
    }
}

fn write_map_header(writer: &mut Vec<u8>, n: usize) {
    if n < 16 {
        writer.push(0x80 | (n as u8));
    } else if u16::try_from(n).is_ok() {
        writer.push(0xde);
        writer.extend_from_slice(&(n as u16).to_be_bytes());
    } else {
        // MessagePack's `map 32` header caps length at `u32::MAX`. On
        // 64-bit hosts a `usize` could in principle exceed that; use
        // a checked conversion so we panic with a clear message
        // rather than silently truncating to a corrupt payload.
        let n = u32::try_from(n).expect("map length exceeds MessagePack's u32::MAX limit");
        writer.push(0xdf);
        writer.extend_from_slice(&n.to_be_bytes());
    }
}

fn write_str(writer: &mut Vec<u8>, text: &str) {
    let bytes = text.as_bytes();
    let n = bytes.len();
    if n < 32 {
        writer.push(0xa0 | (n as u8));
    } else if u8::try_from(n).is_ok() {
        writer.push(0xd9);
        writer.push(n as u8);
    } else if u16::try_from(n).is_ok() {
        writer.push(0xda);
        writer.extend_from_slice(&(n as u16).to_be_bytes());
    } else {
        // `str 32` tops out at `u32::MAX` bytes. Checked cast to
        // fail loudly rather than silently truncating to a corrupt
        // length prefix.
        let n = u32::try_from(n).expect("string length exceeds MessagePack's u32::MAX limit");
        writer.push(0xdb);
        writer.extend_from_slice(&n.to_be_bytes());
    }
    writer.extend_from_slice(bytes);
}

#[cfg(test)]
mod tests;

mod encoding;
