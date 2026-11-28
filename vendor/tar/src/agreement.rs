//! Reads header shapes the way node-tar does, or rejects them.
//!
//! npm, Yarn, and Bun read packages with node-tar. When the two readers
//! disagree on an entry's size or path, or on where the next header starts,
//! one tarball holds different files for each of them. The readers here
//! decode a field as node-tar does. Each check rejects one shape that this
//! crate does not read that way yet.

use std::{
    collections::{
        HashMap,
        HashSet,
    },
    io,
    str,
};

use crate::{
    Header,
    other,
    pax::{
        PAX_LINKPATH,
        PAX_PATH,
        PAX_SIZE,
    },
};

/// node-tar's default `maxMetaEntrySize`. node-tar skips a larger GNU
/// long-name, GNU long-link, or PAX entry without applying it.
pub(crate) const NODE_TAR_MAX_META_ENTRY_SIZE: u64 = 1024 * 1024;

/// `Number.MAX_SAFE_INTEGER`, the largest size node-tar reads exactly.
const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

/// The typeflags node-tar applies extended headers to.
pub(crate) fn is_normal_fs_type(typeflag: u8) -> bool {
    matches!(typeflag, b'\0' | b'0'..=b'7' | b'D')
}

fn is_link_type(typeflag: u8) -> bool {
    matches!(typeflag, b'1' | b'2')
}

fn is_pax_type(typeflag: u8) -> bool {
    matches!(typeflag, b'x' | b'g')
}

/// Whether `typeflag` names a GNU long name header. node-tar reads an old
/// GNU `N` header as one.
pub(crate) fn is_gnu_long_name_type(typeflag: u8) -> bool {
    matches!(typeflag, b'L' | b'N')
}

/// Checks one header block on its own, before its size is used.
///
/// node-tar skips a header block with no path, or with a link name on an
/// entry that is not a link, and parses the next block as a header. It reads
/// a Solaris `X` header as a PAX header, which this crate does not.
///
/// The path and link name are the ones the header's own fields hold.
pub(crate) fn check_header_block(
    header: &Header,
    (path, link_name): (&[u8], Option<&[u8]>),
) -> io::Result<()> {
    let typeflag = header.as_old().linkflag[0];
    if typeflag == b'X' {
        return Err(other("Solaris extended headers are not supported"));
    }
    if !is_normal_fs_type(typeflag) && path.is_empty() {
        return Err(other("archive header has no path"));
    }
    if !is_link_type(typeflag) && !is_pax_type(typeflag) && link_name.is_some() {
        return Err(entry_error(path, "is not a link but has a link name"));
    }
    Ok(())
}

/// Whether node-tar reads `block` as a header. It skips a block whose
/// checksum does not match, or whose numbers it cannot decode, and parses
/// the next block as a header.
///
/// node-tar reads the checksum from a 12-byte field that runs on into the
/// typeflag and the link name.
pub(crate) fn is_valid_header_block(block: &[u8; 512]) -> bool {
    let Ok(Some(stored)) = node_tar_number(&block[148..160]) else {
        return false;
    };
    stored == header_block_sum(block)
}

/// Whether node-tar reads `block` as one of the two blocks of the
/// end-of-archive marker: no checksum, and zero bytes outside it.
pub(crate) fn is_null_block(block: &[u8; 512]) -> bool {
    matches!(node_tar_number(&block[148..160]), Ok(None)) && header_block_sum(block) == 8 * 32
}

/// The checksum of `block`, counting its 8-byte checksum field as spaces.
fn header_block_sum(block: &[u8; 512]) -> i64 {
    block[..148]
        .iter()
        .chain(&block[156..])
        .map(|byte| i64::from(*byte))
        .sum::<i64>()
        + 8 * 32
}

/// The entry size in a header's `size` field, as node-tar reads it. A
/// field it reads as no number, or as a negative one, means no data.
/// `None` when node-tar cannot decode the field, and skips the block.
pub(crate) fn header_size(field: &[u8]) -> Option<u64> {
    let size = node_tar_number(field).ok()?;
    Some(
        size.and_then(|size| u64::try_from(size).ok())
            .unwrap_or(0),
    )
}

/// A numeric header field as node-tar decodes it. `Ok(None)` is a field it
/// reads as no number. An error is a field it cannot decode.
///
/// A field whose first byte has its high bit set is base-256: `0x80` starts
/// a positive number and `0xff` a negative one. Otherwise node-tar decodes
/// the field as UTF-8 and cuts it with `/\0.*$/`. It then trims it with
/// JavaScript's `trim` and reads it with `parseInt(text, 8)`, which reads
/// the leading octal digits after an optional sign.
fn node_tar_number(field: &[u8]) -> Result<Option<i64>, ()> {
    if field[0] & 0x80 != 0 {
        return base256_number(field).map(Some);
    }
    let text = String::from_utf8_lossy(field);
    let after_last_line = text.rfind(is_line_terminator).unwrap_or(0);
    let text = match text[after_last_line..].find('\0') {
        Some(nul) => &text[..after_last_line + nul],
        None => &text[..],
    };
    let text = text.trim_matches(is_javascript_white_space);
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let digits = &digits[..digits
        .find(|ch: char| !matches!(ch, '0'..='7'))
        .unwrap_or(digits.len())];
    if digits.is_empty() {
        return Ok(None);
    }
    let value = i64::from_str_radix(digits, 8).map_err(|_| ())?;
    Ok(Some(if negative { -value } else { value }))
}

/// node-tar's base-256 reader. It reads every byte after the marker.
fn base256_number(field: &[u8]) -> Result<i64, ()> {
    let negative = match field[0] {
        0x80 => false,
        0xff => true,
        _ => return Err(()),
    };
    let bytes = if negative { field } else { &field[1..] };
    let mut magnitude: u64 = 0;
    for byte in bytes {
        let byte = if negative { !byte } else { *byte };
        magnitude = magnitude
            .checked_mul(256)
            .and_then(|value| value.checked_add(u64::from(byte)))
            .filter(|value| *value <= MAX_SAFE_INTEGER)
            .ok_or(())?;
    }
    if negative {
        // `!byte` drops the 1 that two's complement adds.
        let magnitude = magnitude
            .checked_add(1)
            .filter(|value| *value <= MAX_SAFE_INTEGER)
            .ok_or(())?;
        return i64::try_from(magnitude)
            .map(|value| -value)
            .map_err(|_| ());
    }
    i64::try_from(magnitude).map_err(|_| ())
}

/// JavaScript's white space and line terminators, which `trim` removes.
fn is_javascript_white_space(ch: char) -> bool {
    matches!(
        ch,
        '\t' | '\u{b}' | '\u{c}' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
    ) || is_line_terminator(ch)
}

/// The line terminators of a JavaScript regular expression, which `.` does
/// not match.
fn is_line_terminator(ch: char) -> bool {
    matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// A text field as node-tar decodes it: UTF-8, cut by `/\0.*/`. The cut
/// runs from the first NUL to the next line terminator, which `.` does not
/// match, and keeps the rest.
pub(crate) fn node_tar_string(field: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(field);
    let Some(nul) = text.find('\0') else {
        return text.into_owned().into_bytes();
    };
    let rest = &text[nul..];
    let kept = rest
        .find(is_line_terminator)
        .map_or("", |end| &rest[end..]);
    [&text[..nul], kept].concat().into_bytes()
}

/// The path and link name in a header's own fields, as node-tar reads them.
///
/// node-tar joins the ustar prefix to the name whenever the prefix field's
/// last byte is set, even when the prefix is empty. Otherwise it reads the
/// prefix from its first 130 bytes and joins it when it is not empty.
pub(crate) fn header_names(header: &Header) -> (Vec<u8>, Option<Vec<u8>>) {
    let old = header.as_old();
    let mut path = node_tar_string(&old.name);
    if let Some(ustar) = header.as_ustar() {
        let joined = ustar.prefix[130] != 0;
        let prefix = node_tar_string(if joined { &ustar.prefix } else { &ustar.prefix[..130] });
        if joined || !prefix.is_empty() {
            path = [&prefix[..], b"/", &path[..]].concat();
        }
    }
    let link_name = Some(node_tar_string(&old.linkname)).filter(|name| !name.is_empty());
    (path, link_name)
}

fn entry_error(path: &[u8], problem: &str) -> io::Error {
    other(&format!("archive entry {:?} {}", String::from_utf8_lossy(path), problem))
}

/// Checks the size of an entry that is not an extension header.
///
/// node-tar reads a directory as having no data whatever its size, and
/// treats the bytes after a sized link or device entry as its data.
pub(crate) fn check_entry_size(header: &Header, size: u64) -> io::Result<()> {
    if size != 0 && matches!(header.as_old().linkflag[0], b'1'..=b'6') {
        return Err(entry_error(&header.path_bytes(), "cannot hold data but has a nonzero size"));
    }
    Ok(())
}

/// Checks a PAX extended header payload.
///
/// node-tar skips a malformed record and reads the rest. It lets the last
/// of two records with one key win, cuts a value at its first NUL, and
/// decodes the payload as UTF-8. This crate stops at a malformed record and
/// lets the first of two records win. A global header's `size` applies to
/// every later entry in node-tar and to none in this crate.
pub(crate) fn check_pax_records(payload: &[u8], global: bool) -> io::Result<()> {
    let text = str::from_utf8(payload).map_err(|_| other("PAX extended header is not UTF-8"))?;
    let mut keys = HashSet::new();
    let mut rest = text;
    while !rest.is_empty() {
        let (key, value, len) = parse_pax_record(rest, false)?;
        if !keys.insert(key) {
            return Err(other(&format!("PAX extended header sets `{}` more than once", key)));
        }
        if value.contains('\0') {
            return Err(other("PAX extended header value contains a NUL byte"));
        }
        if global && [PAX_SIZE, PAX_PATH, PAX_LINKPATH].contains(&key) {
            return Err(other(&format!("global PAX extended header sets `{}`", key)));
        }
        rest = &rest[len..];
    }
    Ok(())
}

/// Drops every PAX record that a later record with the same key
/// overrides, as node-tar does.
pub(crate) fn keep_last_pax_records(payload: &[u8]) -> io::Result<Vec<u8>> {
    let text = str::from_utf8(payload).map_err(|_| other("PAX extended header is not UTF-8"))?;
    let mut records = Vec::new();
    let mut last_index = HashMap::new();
    let mut rest = text;
    while !rest.is_empty() {
        let (key, value, len) = parse_pax_record(rest, true)?;
        if value.contains('\n') {
            rest = &rest[len..];
            continue;
        }
        last_index.insert(key, records.len());
        records.push((key, &rest[..len]));
        rest = &rest[len..];
    }
    Ok(records
        .iter()
        .enumerate()
        .filter(|(index, (key, _))| last_index[key] == *index)
        .map(|(_, (_, record))| *record)
        .collect::<String>()
        .into_bytes())
}

/// Parses the first `<length> <key>=<value>\n` record of `text`, returning
/// its key, its value, and its length in bytes.
///
/// The length has no leading zero because node-tar finds the key by the
/// width of the number it parsed, not by the position of the space.
fn parse_pax_record(text: &str, read_like_node_tar: bool) -> io::Result<(&str, &str, usize)> {
    let malformed = || other("malformed PAX extended header record");
    let (digits, _) = text.split_once(' ').ok_or_else(malformed)?;
    if digits.is_empty()
        || !digits.bytes().all(|byte| byte.is_ascii_digit())
        || (digits.len() > 1 && digits.starts_with('0'))
    {
        return Err(malformed());
    }
    let len: usize = digits.parse().map_err(|_| malformed())?;
    let record = text.get(..len).ok_or_else(malformed)?;
    let key_value = record
        .get(digits.len() + 1..)
        .and_then(|rest| rest.strip_suffix('\n'))
        .ok_or_else(malformed)?;
    let (key, value) = key_value.split_once('=').ok_or_else(malformed)?;
    if key.is_empty() || key.contains('\n') {
        return Err(malformed());
    }
    if value.contains('\n') && !(read_like_node_tar && ignored_multiline_metadata(key, value)) {
        return Err(malformed());
    }
    Ok((key, value, len))
}

/// node-tar reads physical lines. The first line is shorter than the declared
/// record length. Skip the record only if no continuation can start with a
/// decimal length after JavaScript whitespace and an optional sign.
fn ignored_multiline_metadata(key: &str, value: &str) -> bool {
    key.starts_with("NODETAR.package.")
        && !value.contains('\0')
        && value
            .split('\n')
            .skip(1)
            .all(|line| {
                let line =
                    line.trim_start_matches(|ch: char| ch.is_whitespace() || ch == '\u{feff}');
                let digits = line
                    .strip_prefix(['+', '-'])
                    .unwrap_or(line);
                !digits.starts_with(|ch: char| ch.is_ascii_digit())
            })
}

/// Reads the `size` record of a payload that [`check_pax_records`] accepted.
///
/// node-tar converts the value with JavaScript's `Number`, which accepts
/// spaces, a fraction, an exponent, or hex, and reads an empty value as 0.
/// Only plain decimal digits mean the same number to both readers.
pub(crate) fn pax_size(payload: &[u8]) -> io::Result<Option<u64>> {
    let Ok(mut rest) = str::from_utf8(payload) else {
        return Ok(None);
    };
    while !rest.is_empty() {
        let (key, value, len) = parse_pax_record(rest, false)?;
        if key == PAX_SIZE {
            return parse_pax_size(value).map(Some);
        }
        rest = &rest[len..];
    }
    Ok(None)
}

fn parse_pax_size(value: &str) -> io::Result<u64> {
    let invalid = || other(&format!("PAX extended header has an invalid size `{}`", value));
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid());
    }
    value
        .parse::<u64>()
        .ok()
        .filter(|size| *size <= MAX_SAFE_INTEGER)
        .ok_or_else(invalid)
}

/// Checks the base-256 form of a header's size field.
///
/// node-tar reads every byte after the marker and reads a leading `0xff` as
/// a negative number. This crate reads only the last eight bytes, as a
/// positive number.
pub(crate) fn check_base256_size(field: &[u8]) -> io::Result<()> {
    let invalid = || other("archive header has an invalid base-256 size");
    if field[0] != 0x80 {
        return Err(invalid());
    }
    let mut value: u64 = 0;
    for byte in &field[1..] {
        value = value
            .checked_mul(256)
            .and_then(|value| value.checked_add(u64::from(*byte)))
            .filter(|value| *value <= MAX_SAFE_INTEGER)
            .ok_or_else(invalid)?;
    }
    Ok(())
}

/// Checks the name and link name an entry ends up with.
///
/// node-tar lets the later of a GNU long name and a PAX `path` win, while
/// this crate always prefers the long name; likewise for the link name. It
/// reads a regular file whose path ends in `/` as a directory with no data.
pub(crate) fn check_resolved_entry(
    typeflag: u8,
    size: u64,
    path: &[u8],
    link_name: Option<&[u8]>,
    has_long_and_pax_name: bool,
) -> io::Result<()> {
    if has_long_and_pax_name {
        return Err(entry_error(path, "is named by both a GNU long header and a PAX header"));
    }
    if path.is_empty() {
        return Err(other("archive entry has no path"));
    }
    // Node.js refuses a path with a NUL byte, so npm fails on the entry.
    if path.contains(&0) {
        return Err(entry_error(path, "has a NUL byte in its path"));
    }
    let has_link_name = link_name.is_some_and(|name| !name.is_empty());
    if is_link_type(typeflag) != has_link_name {
        return Err(entry_error(
            path,
            if has_link_name {
                "is not a link but has a link name"
            } else {
                "is a link without a link name"
            },
        ));
    }
    if matches!(typeflag, b'\0' | b'0') && size != 0 && path.ends_with(b"/") {
        return Err(entry_error(path, "is a regular file ending in `/` with data"));
    }
    Ok(())
}
