//! Encoder and decoder for the narrow subset of
//! [msgpackr](https://github.com/kriszyp/msgpackr)'s wire format that
//! pnpm v11 uses to write `index.db` rows — standard `MessagePack`
//! extended with msgpackr's **records** extension.
//!
//! ## Why this exists
//!
//! pnpm packs every `PackageFilesIndex` with `new Packr({ useRecords: true,
//! moreTypes: true })` (see
//! [`store/index/src/index.ts`](https://github.com/pnpm/pnpm/blob/1819226b51/store/index/src/index.ts)
//! line 12). `useRecords` replaces repeated string keys in same-shape
//! structs with a compact slot reference — roughly, Protobuf field numbers
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
    #[diagnostic(code(pacquet_store_dir::msgpackr_records::unexpected_eof))]
    UnexpectedEof { offset: usize },

    #[display(
        "Reference to unknown record slot 0x{slot:02x} at offset {offset} — \
         the definition was missing or appeared later than its use"
    )]
    #[diagnostic(code(pacquet_store_dir::msgpackr_records::unknown_slot))]
    UnknownSlot { slot: u8, offset: usize },

    #[display(
        "Record definition at offset {offset} has slot 0x{slot:02x}, which \
         is outside the valid reference range 0x40..=0x7f — any reference \
         written for this slot would be unreachable"
    )]
    #[diagnostic(code(pacquet_store_dir::msgpackr_records::slot_out_of_range))]
    SlotOutOfRange { slot: u8, offset: usize },

    #[display(
        "Expected a msgpack array header (fixarray, array16, or array32) \
         for a record-definition field-name list at offset {offset}, got \
         byte 0x{byte:02x}"
    )]
    #[diagnostic(code(pacquet_store_dir::msgpackr_records::expected_array_header))]
    ExpectedArrayHeader { byte: u8, offset: usize },

    #[display(
        "Expected a msgpack string header (fixstr, str8, str16, or str32) \
         for a record-definition field name at offset {offset}, got byte \
         0x{byte:02x}"
    )]
    #[diagnostic(code(pacquet_store_dir::msgpackr_records::expected_string_header))]
    ExpectedStringHeader { byte: u8, offset: usize },

    #[display(
        "Field name in a record definition at offset {offset} contains \
         invalid UTF-8"
    )]
    #[diagnostic(code(pacquet_store_dir::msgpackr_records::invalid_field_name_utf8))]
    InvalidFieldNameUtf8 { offset: usize },

    #[display("Unsupported msgpack header byte 0x{byte:02x} at offset {offset}")]
    #[diagnostic(code(pacquet_store_dir::msgpackr_records::unsupported))]
    Unsupported { byte: u8, offset: usize },

    #[display("{count} bytes left over after decoding the top-level value")]
    #[diagnostic(code(pacquet_store_dir::msgpackr_records::trailing_bytes))]
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

/// Parser context threaded through `transcode_value`. Records mode
/// starts off and flips on the first record definition — msgpackr
/// doesn't re-emit positive fixints in the slot-byte range once records
/// mode is on, so the flip is one-way for any real stream.
///
/// Slot schemas live under `Rc<[String]>` so reference-path decoding
/// can bump a refcount instead of deep-cloning the field-name vector
/// on every record instance. A row with 200 files used to allocate
/// 200 `Vec<String>`s plus one `String` per field name per clone; now
/// it allocates once at definition time.
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
        write_map_header(writer, fields.len());
        for name in fields.iter() {
            write_str(writer, name);
            transcode_value(reader, writer, state)?;
        }
        return Ok(());
    }

    // Record definition — fixext1 with ext type 0x72. Followed by the field-name
    // array, then the first instance inlined. Seeing this header flips the
    // stream into records mode from here on.
    if head == 0xd4 && reader.peek(1)? == RECORD_DEF_EXT_TYPE {
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
        write_map_header(writer, fields.len());
        for name in fields.iter() {
            write_str(writer, name);
            transcode_value(reader, writer, state)?;
        }
        return Ok(());
    }

    // Everything else: vanilla MessagePack. For scalars we just copy the
    // header + payload bytes across; for containers we emit the header and
    // recurse so any records inside still get expanded.
    match head {
        // Positive fixint 0x00..=0x7f. When records mode is active the
        // 0x40..=0x7f slice is trapped above; when it isn't, those bytes
        // are legitimate fixints and pass through.
        0x00..=0x7f => copy_n(reader, writer, 1),

        // Fixmap 0x80..=0x8f
        0x80..=0x8f => {
            let n = (head & 0x0f) as usize;
            reader.read_u8()?;
            writer.push(head);
            transcode_pairs(reader, writer, state, n)
        }
        // Fixarray 0x90..=0x9f
        0x90..=0x9f => {
            let n = (head & 0x0f) as usize;
            reader.read_u8()?;
            writer.push(head);
            transcode_array(reader, writer, state, n)
        }
        // Fixstr 0xa0..=0xbf
        0xa0..=0xbf => {
            let n = (head & 0x1f) as usize;
            copy_n(reader, writer, 1 + n)
        }
        // Negative fixint 0xe0..=0xff
        0xe0..=0xff => copy_n(reader, writer, 1),

        0xc0 /* nil */ | 0xc2 /* false */ | 0xc3 /* true */ => copy_n(reader, writer, 1),

        0xc4 /* bin 8  */ => {
            let n = reader.peek(1)? as usize;
            copy_n(reader, writer, 2 + n)
        }
        0xc5 /* bin 16 */ => {
            let n = u16::from_be_bytes([reader.peek(1)?, reader.peek(2)?]) as usize;
            copy_n(reader, writer, 3 + n)
        }
        0xc6 /* bin 32 */ => {
            let n = u32::from_be_bytes([reader.peek(1)?, reader.peek(2)?, reader.peek(3)?, reader.peek(4)?]) as usize;
            copy_n(reader, writer, 5 + n)
        }

        // ext 8/16/32 — we've handled records above via fixext1; any other ext
        // just passes through. If a future pnpm release sends something fancier
        // we'll see it here.
        0xc7 => {
            let n = reader.peek(1)? as usize;
            copy_n(reader, writer, 3 + n)
        }
        0xc8 => {
            let n = u16::from_be_bytes([reader.peek(1)?, reader.peek(2)?]) as usize;
            copy_n(reader, writer, 4 + n)
        }
        0xc9 => {
            let n = u32::from_be_bytes([reader.peek(1)?, reader.peek(2)?, reader.peek(3)?, reader.peek(4)?]) as usize;
            copy_n(reader, writer, 6 + n)
        }

        // msgpackr emits JS Number as float 64 whenever the value exceeds
        // int32 range — so timestamps like `checkedAt = 1_700_000_000_000`
        // arrive as `cb` + 8 bytes, even though they're semantically
        // integers. `rmp_serde` rejects floats for our integer-typed
        // fields (`size: u64`, `checked_at: Option<u64>`), so narrow
        // the representation back to uint 64 whenever the float is a
        // finite, non-negative integer value that fits. Non-integer or
        // out-of-range floats pass through unchanged so legitimate
        // floats (none appear in `PackageFilesIndex` today, but future
        // fields might) still round-trip.
        0xca /* float 32 */ => {
            reader.read_u8()?;
            let bits = reader.read_bytes(4)?;
            let value = f32::from_be_bytes([bits[0], bits[1], bits[2], bits[3]]);
            maybe_narrow_float_to_uint(writer, f64::from(value), 0xca, &[bits[0], bits[1], bits[2], bits[3]]);
            Ok(())
        }
        0xcb /* float 64 */ => {
            reader.read_u8()?;
            let bits = reader.read_bytes(8)?;
            let arr = [bits[0], bits[1], bits[2], bits[3], bits[4], bits[5], bits[6], bits[7]];
            let value = f64::from_be_bytes(arr);
            maybe_narrow_float_to_uint(writer, value, 0xcb, &arr);
            Ok(())
        }
        0xcc /* uint 8 */   => copy_n(reader, writer, 2),
        0xcd /* uint 16 */  => copy_n(reader, writer, 3),
        0xce /* uint 32 */  => copy_n(reader, writer, 5),
        0xcf /* uint 64 */  => copy_n(reader, writer, 9),
        0xd0 /* int 8 */    => copy_n(reader, writer, 2),
        0xd1 /* int 16 */   => copy_n(reader, writer, 3),
        0xd2 /* int 32 */   => copy_n(reader, writer, 5),
        0xd3 /* int 64 */   => copy_n(reader, writer, 9),

        // fixext 1/2/4/8/16 — 1 ext-type byte + 2^k payload bytes. 0xd4 + type
        // 0x72 is already handled above as records.
        0xd4 => copy_n(reader, writer, 1 + 1 + 1),
        0xd5 => copy_n(reader, writer, 1 + 1 + 2),
        0xd6 => copy_n(reader, writer, 1 + 1 + 4),
        0xd7 => copy_n(reader, writer, 1 + 1 + 8),
        0xd8 => copy_n(reader, writer, 1 + 1 + 16),

        0xd9 /* str 8  */ => {
            let n = reader.peek(1)? as usize;
            copy_n(reader, writer, 2 + n)
        }
        0xda /* str 16 */ => {
            let n = u16::from_be_bytes([reader.peek(1)?, reader.peek(2)?]) as usize;
            copy_n(reader, writer, 3 + n)
        }
        0xdb /* str 32 */ => {
            let n = u32::from_be_bytes([reader.peek(1)?, reader.peek(2)?, reader.peek(3)?, reader.peek(4)?]) as usize;
            copy_n(reader, writer, 5 + n)
        }

        // array 16 / 32 — emit header, recurse N times.
        0xdc => {
            let n = u16::from_be_bytes([reader.peek(1)?, reader.peek(2)?]) as usize;
            writer.extend_from_slice(reader.read_bytes(3)?);
            transcode_array(reader, writer, state, n)
        }
        0xdd => {
            let n = u32::from_be_bytes([reader.peek(1)?, reader.peek(2)?, reader.peek(3)?, reader.peek(4)?]) as usize;
            writer.extend_from_slice(reader.read_bytes(5)?);
            transcode_array(reader, writer, state, n)
        }
        // map 16 / 32
        0xde => {
            let n = u16::from_be_bytes([reader.peek(1)?, reader.peek(2)?]) as usize;
            writer.extend_from_slice(reader.read_bytes(3)?);
            transcode_pairs(reader, writer, state, n)
        }
        0xdf => {
            let n = u32::from_be_bytes([reader.peek(1)?, reader.peek(2)?, reader.peek(3)?, reader.peek(4)?]) as usize;
            writer.extend_from_slice(reader.read_bytes(5)?);
            transcode_pairs(reader, writer, state, n)
        }

        // 0xc1 is reserved in the spec — reject rather than silently drop.
        other => Err(DecodeError::Unsupported { byte: other, offset: start }),
    }
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

/// Encode a [`PackageFilesIndex`] to msgpackr-records bytes that match
/// pnpm v11's wire format closely enough that `Packr({useRecords: true,
/// moreTypes: true}).unpack(bytes)` decodes to the same JS shape pnpm
/// produces itself.
///
/// ## Why not `rmp_serde::to_vec_named`?
///
/// `rmp_serde` emits plain `MessagePack` — every struct becomes a `fixmap`
/// / `map16` / `map32`. That's a perfectly valid `MessagePack` encoding,
/// but msgpackr with `useRecords: true` interprets *every* msgpack map
/// (no matter the nesting depth) as a JS `Map` object, including the
/// top-level `PackageFilesIndex`. pnpm's reader then does
/// `pkgIndex.files` (a property access) on what is actually a `Map`,
/// gets `undefined`, and crashes with `files is not iterable`.
///
/// pnpm itself sidesteps this because it packs the outer struct with
/// `useRecords: true`, which makes msgpackr emit a **record**: the
/// `d4 72 <slot>` fixext1 header followed by a field-name array and the
/// values. Records decode back as plain JS objects, while legitimate JS
/// `Map` values (pnpm's `files` / `sideEffects` / `added`) are still
/// encoded as msgpack maps and decode back as `Map`. The decoder can
/// tell the two apart because records are marked with the fixext1
/// envelope; plain maps aren't.
///
/// So to interop with pnpm, pacquet has to emit records for the Rust
/// `struct`s (object-shape on the pnpm side) and keep plain msgpack
/// maps for the Rust `HashMap`s (`Map`-shape on the pnpm side). That's
/// what this encoder does.
///
/// ## Slot allocation
///
/// Slot `0x40` is reserved for the top-level [`PackageFilesIndex`] —
/// one per row, always first in the stream. Inner slots in
/// `0x41..=0x7f` are allocated **lazily, in first-seen order, one per
/// distinct record shape** (where "shape" is the set of fields that
/// instance actually carries). A single Rust type may therefore span
/// multiple slots if different optional-field combinations show up in
/// the same row: a `CafsFileInfo` carrying `checkedAt` lands in one
/// slot and a `CafsFileInfo` without it lands in another. Same-shape
/// instances downstream collapse to a single bare-slot byte, which is
/// the record-compression win records exist for.
///
/// This is what msgpackr itself does for the same traversal and shape
/// set, so pacquet's output is **wire-compatible** with msgpackr (same
/// record schemas, same slot numbers, same value encodings) — pnpm's
/// reader reconstructs the same JS shape from both. Exact bytes can
/// still differ when Rust's `HashMap` iterates `files` / `sideEffects`
/// / `added` entries in a different order than msgpackr's JS `Map`
/// iteration, which is fine for correctness but worth keeping in mind
/// when diffing bytes against a pnpm-written reference row.
///
/// ## Optional-field handling
///
/// - **`PackageFilesIndex`**: `algo` and `files` are always emitted;
///   `requires_build`, `manifest`, and `side_effects` are included
///   in the record schema only when `Some`. The `manifest`
///   ([`serde_json::Value`]) is encoded recursively, with every
///   nested JSON object record-encoded so a pnpm reader sees them as
///   JS `Object`s (which `manifest.bin` / `manifest.directories?.bin`
///   property access can reach) rather than plain msgpack maps
///   (which msgpackr decodes as JS `Map`s, leaving those property
///   reads `undefined`).
/// - **`CafsFileInfo`**: optional `checkedAt` is omitted from the
///   record schema entirely when `None` rather than written as `nil`,
///   so the presence of `checkedAt` determines the shape and thus
///   the slot. When `Some`, it's written as `float 64` (see
///   [`CafsFileInfo::checked_at`] for why — msgpackr reads `uint 64`
///   as `BigInt`, which crashes pnpm's `mtimeMs - (checkedAt ?? 0)`).
/// - **`SideEffectsDiff`**: `added` and `deleted` are both optional;
///   each is included in the schema only when `Some`. The four
///   possible shapes (`{added}`, `{deleted}`, `{added, deleted}`,
///   `{}`) each get their own slot on first use.
///
/// Matching msgpackr's omit-when-absent convention (rather than
/// padding with `nil`) means pnpm's reader sees the same JS object
/// shape regardless of which tool wrote the row — a `SideEffectsDiff
/// { added: Some, deleted: None }` decodes to `{ added: Map }`, not
/// `{ added: Map, deleted: null }`.
pub fn encode_package_files_index(index: &PackageFilesIndex) -> Result<Vec<u8>, EncodeError> {
    let mut state = EncodeState::default();
    let mut out = Vec::with_capacity(256);
    encode_pkg_files_index_value(&mut out, &mut state, index)?;
    Ok(out)
}

/// Error type of [`encode_package_files_index`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum EncodeError {
    #[display(
        "Ran out of msgpackr record slots: encountered more than \
         {max} distinct record shapes (slot range is 0x41..=0x7f). \
         `CafsFileInfo` contributes at most 2 shapes and \
         `SideEffectsDiff` at most 4; the rest are allocated lazily \
         from `PackageFilesIndex.manifest`'s nested object shapes, \
         which in practice fit comfortably inside the remaining \
         range for a single tarball's manifest. Reaching this error \
         likely means the encoder is being reused for a payload it \
         wasn't designed for."
    )]
    #[diagnostic(code(pacquet_store_dir::msgpackr_records::out_of_record_slots))]
    OutOfRecordSlots { max: usize },
}

/// Slot allocated to the top-level [`PackageFilesIndex`] record.
/// A single stream always has exactly one of these, so it gets the
/// base slot. Inner records (`CafsFileInfo`, `SideEffectsDiff`) are
/// allocated lazily from `FIRST_INNER_SLOT` upwards, one slot per
/// distinct shape — see [`EncodeState::allocate_slot`].
const PKG_FILES_INDEX_SLOT: u8 = SLOT_LO; // 0x40
const FIRST_INNER_SLOT: u8 = SLOT_LO + 1; // 0x41

/// Tracks which shapes have been defined and what slot each got.
/// Mirrors msgpackr's own strategy: when it sees a new record instance
/// whose field set differs from anything previously packed, it
/// allocates a new slot rather than redefining an existing one — so
/// same-shape instances downstream collapse to a single bare-slot byte
/// (the point of records), and mixed-shape streams still decode
/// correctly without per-instance re-defs.
///
/// Shape keys are small bitmasks over the optional fields of each
/// record type, see `cafs_shape` / `side_effects_shape`. Each type has
/// at most a handful of possible shapes (2 for `CafsFileInfo`, 4 for
/// `SideEffectsDiff`), so the 0x40..=0x7f slot range is vastly
/// over-provisioned for realistic workloads.
#[derive(SmartDefault)]
struct EncodeState {
    /// Shape → slot for every `CafsFileInfo` shape seen so far. The
    /// index is the shape bitmask produced by [`cafs_shape`] (2
    /// possible values today). `None` = shape hasn't been emitted yet
    /// in this stream.
    cafs_slots: [Option<u8>; 2],
    /// Same for `SideEffectsDiff`, indexed by [`side_effects_shape`]
    /// (4 possible values).
    side_effects_slots: [Option<u8>; 4],
    /// Field-name vector → slot for every JSON-object shape seen so
    /// far inside a manifest value. Shape keys are owned `Vec<String>`
    /// because the field names are read from a borrowed
    /// `serde_json::Map` whose lifetime ends before the next encode
    /// call wants the lookup. msgpackr does the equivalent thing for
    /// arbitrary JS objects under `useRecords: true`; pacquet has to
    /// match so a pnpm reader sees the manifest's nested objects as JS
    /// `Object`s (record-decoded) rather than `Map`s (plain-msgpack-
    /// decoded), which is what pnpm's bin linker reads with
    /// `manifest.bin` / `manifest.directories?.bin` property access.
    json_object_slots: HashMap<Vec<String>, u8>,
    /// Next unused slot in the 0x41..=0x7f range. Starts above
    /// `PKG_FILES_INDEX_SLOT` because the top-level record always
    /// takes slot 0x40.
    #[default(FIRST_INNER_SLOT)]
    next_slot: u8,
}

impl EncodeState {
    fn allocate_slot(&mut self) -> Result<u8, EncodeError> {
        if self.next_slot > SLOT_HI {
            return Err(EncodeError::OutOfRecordSlots {
                max: (SLOT_HI - FIRST_INNER_SLOT + 1) as usize,
            });
        }
        let slot = self.next_slot;
        self.next_slot += 1;
        Ok(slot)
    }
}

/// Bitmask describing which optional fields a [`CafsFileInfo`] carries.
/// Bit 0 = `checked_at`. Required fields (digest, mode, size) don't
/// affect the shape because they're always present.
fn cafs_shape(info: &CafsFileInfo) -> u8 {
    u8::from(info.checked_at.is_some())
}

/// Bitmask describing which optional fields a [`SideEffectsDiff`]
/// carries. Bit 0 = `added`, bit 1 = `deleted`.
fn side_effects_shape(diff: &SideEffectsDiff) -> u8 {
    u8::from(diff.added.is_some()) | (u8::from(diff.deleted.is_some()) << 1)
}

fn encode_pkg_files_index_value(
    writer: &mut Vec<u8>,
    state: &mut EncodeState,
    idx: &PackageFilesIndex,
) -> Result<(), EncodeError> {
    // Field order `[algo, requiresBuild?, manifest?, files, sideEffects?]`.
    // Optional fields are omitted from the schema when `None`, matching
    // msgpackr's field-omit-when-absent shape so a pnpm reader sees the
    // same JS object regardless of whether pacquet or pnpm wrote the row.
    let mut fields: Vec<&str> = Vec::with_capacity(5);
    fields.push("algo");
    if idx.requires_build.is_some() {
        fields.push("requiresBuild");
    }
    if idx.manifest.is_some() {
        fields.push("manifest");
    }
    fields.push("files");
    if idx.side_effects.is_some() {
        fields.push("sideEffects");
    }

    write_record_def_header(writer, PKG_FILES_INDEX_SLOT, &fields);

    // Values in the same order as `fields` above.
    write_str(writer, &idx.algo);
    if let Some(rb) = idx.requires_build {
        write_bool(writer, rb);
    }
    if let Some(manifest) = &idx.manifest {
        encode_json_value(writer, state, manifest)?;
    }
    // Iterate the file map in sorted-key order so the emitted
    // msgpack bytes are byte-stable across runs. `HashMap`'s
    // iteration is randomised, which would make every row pacquet
    // writes appear "changed" on byte-diff even when the logical
    // content is identical. Sorting here matches what msgpackr-on-
    // JS effectively delivers via `Object` insertion order on
    // deterministic input (npm tarballs walk files in directory
    // order, which is sorted on most filesystems).
    write_map_header(writer, idx.files.len());
    for (name, info) in sorted_by_key(&idx.files) {
        write_str(writer, name);
        encode_cafs_file_info(writer, state, info)?;
    }
    if let Some(se) = &idx.side_effects {
        write_map_header(writer, se.len());
        for (platform, diff) in sorted_by_key(se) {
            write_str(writer, platform);
            encode_side_effects_diff(writer, state, diff)?;
        }
    }

    Ok(())
}

/// Sort a `HashMap` by key into a `Vec` of `(key, value)`
/// references. Used by the msgpackr-records encoder so every map
/// it writes — `PackageFilesIndex.files`, `…side_effects`,
/// `SideEffectsDiff.added` — comes out in lexicographic key
/// order. Without this the row payload depends on
/// `HashMap`'s randomised iteration and isn't reproducible.
fn sorted_by_key<Value>(map: &HashMap<String, Value>) -> Vec<(&String, &Value)> {
    let mut entries: Vec<(&String, &Value)> = map.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    entries
}

/// Emit one JSON value as msgpack inside an active records stream.
/// Scalars use the smallest slot-safe encoding (no bare positive
/// fixints in `0x40..=0x7f`, which would otherwise be misread as
/// record-slot references — see [`write_uint`]). Arrays are plain
/// msgpack `fixarray` / `array16` / `array32`. Objects are
/// **record-encoded** via [`encode_json_object`] so that
/// `useRecords: true` decoders see them as JS `Object` rather than
/// JS `Map` — necessary for pnpm's bin linker to find
/// `manifest.bin` / `manifest.directories?.bin` via property
/// access.
fn encode_json_value(
    writer: &mut Vec<u8>,
    state: &mut EncodeState,
    value: &Value,
) -> Result<(), EncodeError> {
    match value {
        Value::Null => writer.push(0xc0),
        Value::Bool(b) => write_bool(writer, *b),
        Value::Number(n) => encode_json_number(writer, n),
        Value::String(s) => write_str(writer, s),
        Value::Array(arr) => {
            write_array_header(writer, arr.len());
            for item in arr {
                encode_json_value(writer, state, item)?;
            }
        }
        Value::Object(obj) => encode_json_object(writer, state, obj)?,
    }
    Ok(())
}

/// Record-encode a JSON object: allocate one slot per distinct key
/// set seen in the current stream, emit a record def the first time
/// each shape appears, and emit a bare slot byte on subsequent
/// instances of the same shape. The slot table lives on
/// [`EncodeState::json_object_slots`] so reuse compresses repeated
/// nested-object shapes (e.g. multiple `bin: { command: path }`
/// objects with the same single command name) the same way
/// msgpackr's records mode does.
///
/// Field iteration order is the [`serde_json::Map`]'s own order;
/// pacquet builds with `serde_json/preserve_order`, so that's the
/// insertion order from parsing the original `package.json` — the
/// same order pnpm itself observes when packing the manifest.
fn encode_json_object(
    writer: &mut Vec<u8>,
    state: &mut EncodeState,
    obj: &serde_json::Map<String, Value>,
) -> Result<(), EncodeError> {
    let fields: Vec<String> = obj.keys().cloned().collect();
    if let Some(&slot) = state.json_object_slots.get(&fields) {
        writer.push(slot);
    } else {
        let slot = state.allocate_slot()?;
        let field_refs: Vec<&str> = fields.iter().map(String::as_str).collect();
        write_record_def_header(writer, slot, &field_refs);
        state.json_object_slots.insert(fields, slot);
    }
    for value in obj.values() {
        encode_json_value(writer, state, value)?;
    }
    Ok(())
}

/// Encode a [`serde_json::Number`] using the smallest slot-safe
/// `MessagePack` form. The branch order matches what pnpm's msgpackr
/// itself picks: any integer value first (so a JSON `1.0` parsed as
/// `Number(1)` stays an integer on the wire), falling through to
/// `float 64` only when the number genuinely needs the precision.
fn encode_json_number(writer: &mut Vec<u8>, n: &serde_json::Number) {
    if let Some(u) = n.as_u64() {
        write_uint(writer, u);
        return;
    }
    if let Some(i) = n.as_i64() {
        write_int(writer, i);
        return;
    }
    if let Some(f) = n.as_f64() {
        write_float64(writer, f);
    }
    // Unreachable: a `serde_json::Number` is always one of the three
    // cases above. If it isn't (a future serde_json release adds a
    // new representation), the wire output would be missing a value
    // for this field, which would surface as a deserialize error on
    // round-trip — louder than silent corruption.
}

fn encode_cafs_file_info(
    writer: &mut Vec<u8>,
    state: &mut EncodeState,
    info: &CafsFileInfo,
) -> Result<(), EncodeError> {
    let shape = cafs_shape(info);
    if let Some(slot) = state.cafs_slots[shape as usize] {
        writer.push(slot); // bare slot = record reference; no def needed
    } else {
        // New shape for this stream — allocate a slot and emit a
        // record def inline. `digest`, `mode`, `size` are required;
        // `checkedAt` is included only when `Some`, matching msgpackr's
        // field-omit-when-absent behaviour so pnpm's reader sees the
        // same object shape on round-trip. Field order matches pnpm's
        // own output.
        let slot = state.allocate_slot()?;
        state.cafs_slots[shape as usize] = Some(slot);
        let fields: &[&str] = if info.checked_at.is_some() {
            &["digest", "mode", "size", "checkedAt"]
        } else {
            &["digest", "mode", "size"]
        };
        write_record_def_header(writer, slot, fields);
    }

    write_str(writer, &info.digest);
    write_uint(writer, u64::from(info.mode));
    write_uint(writer, info.size);
    if let Some(v) = info.checked_at {
        // Float 64 — not uint 64 — because msgpackr decodes `uint 64`
        // as a JS `BigInt`, and pnpm's integrity check does
        // `mtimeMs - (checkedAt ?? 0)` which throws `TypeError: Cannot
        // mix BigInt and other types`. Packing as a double matches
        // what pnpm writes for the same millisecond-epoch value (JS
        // Number is a double, so msgpackr emits `cb` + 8 bytes for
        // values past int32 range).
        write_float64(writer, v as f64);
    }
    Ok(())
}

fn encode_side_effects_diff(
    writer: &mut Vec<u8>,
    state: &mut EncodeState,
    diff: &SideEffectsDiff,
) -> Result<(), EncodeError> {
    let shape = side_effects_shape(diff);
    if let Some(slot) = state.side_effects_slots[shape as usize] {
        writer.push(slot);
    } else {
        // Msgpackr omits absent `added` / `deleted` from the schema
        // rather than writing them as explicit `null`. Match that so
        // downstream JS code checking `diff.added != null` /
        // `diff.deleted != null` sees the same shape regardless of
        // which tool wrote the row.
        let slot = state.allocate_slot()?;
        state.side_effects_slots[shape as usize] = Some(slot);
        let fields: &[&str] = match (diff.added.is_some(), diff.deleted.is_some()) {
            (true, true) => &["added", "deleted"],
            (true, false) => &["added"],
            (false, true) => &["deleted"],
            (false, false) => &[],
        };
        write_record_def_header(writer, slot, fields);
    }

    if let Some(added) = &diff.added {
        write_map_header(writer, added.len());
        for (name, info) in sorted_by_key(added) {
            write_str(writer, name);
            encode_cafs_file_info(writer, state, info)?;
        }
    }
    if let Some(deleted) = &diff.deleted {
        write_array_header(writer, deleted.len());
        for name in deleted {
            write_str(writer, name);
        }
    }
    Ok(())
}

/// `d4 72 <slot>` fixext1 header + msgpack array of `fields` as strings.
fn write_record_def_header(writer: &mut Vec<u8>, slot: u8, fields: &[&str]) {
    writer.push(0xd4);
    writer.push(RECORD_DEF_EXT_TYPE);
    writer.push(slot);
    write_array_header(writer, fields.len());
    for field in fields {
        write_str(writer, field);
    }
}

fn write_array_header(writer: &mut Vec<u8>, n: usize) {
    if n < 16 {
        writer.push(0x90 | (n as u8));
    } else if u16::try_from(n).is_ok() {
        writer.push(0xdc);
        writer.extend_from_slice(&(n as u16).to_be_bytes());
    } else {
        // `array 32` tops out at `u32::MAX` entries. Checked cast
        // so an overflow panics with a clear message rather than
        // silently truncating to a corrupt length prefix.
        let n = u32::try_from(n).expect("array length exceeds MessagePack's u32::MAX limit");
        writer.push(0xdd);
        writer.extend_from_slice(&n.to_be_bytes());
    }
}

/// Write an unsigned integer in the smallest `MessagePack` encoding that
/// is safe inside an active records stream. Values `0x40..=0x7f` cannot
/// be emitted as positive fixints — their byte representation collides
/// with record-slot references — so they get promoted to `uint 8`.
/// msgpackr does the same thing under `useRecords: true` for exactly
/// the same reason. `mode: u32` (e.g. `0o755` = 493) and `size: u64`
/// round-trip through this.
fn write_uint(writer: &mut Vec<u8>, value: u64) {
    if value < u64::from(SLOT_LO) {
        // Positive fixint 0x00..=0x3f — below the slot range, safe to
        // emit bare.
        writer.push(value as u8);
    } else if u8::try_from(value).is_ok() {
        // Covers 0x40..=0xff; the 0x40..=0x7f sub-range must use uint 8
        // so the decoder doesn't mistake it for a slot byte.
        writer.push(0xcc);
        writer.push(value as u8);
    } else if u16::try_from(value).is_ok() {
        writer.push(0xcd);
        writer.extend_from_slice(&(value as u16).to_be_bytes());
    } else if u32::try_from(value).is_ok() {
        writer.push(0xce);
        writer.extend_from_slice(&(value as u32).to_be_bytes());
    } else {
        writer.push(0xcf);
        writer.extend_from_slice(&value.to_be_bytes());
    }
}

fn write_float64(writer: &mut Vec<u8>, value: f64) {
    writer.push(0xcb);
    writer.extend_from_slice(&value.to_be_bytes());
}

/// Write a signed integer in the smallest `MessagePack` encoding.
/// Negative values use the int 8/16/32/64 family (`0xd0..=0xd3`),
/// whose header bytes are outside the records-mode slot range so
/// they're always safe to emit. Non-negative values delegate to
/// [`write_uint`] which handles the slot-byte fixint promotion.
fn write_int(writer: &mut Vec<u8>, value: i64) {
    if value >= 0 {
        write_uint(writer, value as u64);
    } else if value >= -32 {
        // Negative fixint `0xe0..=0xff`; outside slot range.
        writer.push(value as i8 as u8);
    } else if value >= i64::from(i8::MIN) {
        writer.push(0xd0);
        writer.push(value as i8 as u8);
    } else if value >= i64::from(i16::MIN) {
        writer.push(0xd1);
        writer.extend_from_slice(&(value as i16).to_be_bytes());
    } else if value >= i64::from(i32::MIN) {
        writer.push(0xd2);
        writer.extend_from_slice(&(value as i32).to_be_bytes());
    } else {
        writer.push(0xd3);
        writer.extend_from_slice(&value.to_be_bytes());
    }
}

fn write_bool(writer: &mut Vec<u8>, value: bool) {
    writer.push(if value { 0xc3 } else { 0xc2 });
}

#[cfg(test)]
mod tests;
