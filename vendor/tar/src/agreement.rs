//! Rejects header shapes that node-tar reads differently from this crate.
//!
//! npm, Yarn, and Bun read packages with node-tar. When the two readers
//! disagree on an entry's size or path, or on where the next header starts,
//! one tarball holds different files for each of them. Each check here
//! rejects one such shape instead of picking a side.

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

/// Checks one header block on its own, before its size is used.
///
/// node-tar skips a header block with no path, or with a link name on an
/// entry that is not a link, and parses the next block as a header. It reads
/// a Solaris `X` header as a PAX header, which this crate does not.
pub(crate) fn check_header_block(header: &Header) -> io::Result<()> {
    let typeflag = header.as_old().linkflag[0];
    if typeflag == b'X' {
        return Err(other("Solaris extended headers are not supported"));
    }
    if !is_normal_fs_type(typeflag) && header.path_bytes().is_empty() {
        return Err(other("archive header has no path"));
    }
    if !is_link_type(typeflag) && !is_pax_type(typeflag) && header.link_name_bytes().is_some() {
        return Err(entry_error(&header.path_bytes(), "is not a link but has a link name"));
    }
    Ok(())
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

/// Checks a GNU long name or long link payload.
///
/// node-tar cuts the value at its first NUL. This crate drops only one
/// trailing NUL.
pub(crate) fn check_gnu_long_value(value: &[u8]) -> io::Result<()> {
    let value = value.strip_suffix(b"\0").unwrap_or(value);
    if value.contains(&0) {
        return Err(other("GNU long name or long link contains a NUL byte"));
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
        let (key, value, len) = parse_pax_record(rest)?;
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
        let (key, _, len) = parse_pax_record(rest)?;
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
fn parse_pax_record(text: &str) -> io::Result<(&str, &str, usize)> {
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
    if key_value.contains('\n') {
        return Err(malformed());
    }
    let (key, value) = key_value.split_once('=').ok_or_else(malformed)?;
    if key.is_empty() {
        return Err(malformed());
    }
    Ok((key, value, len))
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
        let (key, value, len) = parse_pax_record(rest)?;
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
