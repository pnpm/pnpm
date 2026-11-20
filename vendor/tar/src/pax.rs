#![allow(dead_code)]
use std::io;
use std::io::Write;
use std::slice;
use std::str;

use crate::other;

// Keywords for PAX extended header records.
pub const PAX_NONE: &str = ""; // Indicates that no PAX key is suitable
pub const PAX_PATH: &str = "path";
pub const PAX_LINKPATH: &str = "linkpath";
pub const PAX_SIZE: &str = "size";
pub const PAX_UID: &str = "uid";
pub const PAX_GID: &str = "gid";
pub const PAX_UNAME: &str = "uname";
pub const PAX_GNAME: &str = "gname";
pub const PAX_MTIME: &str = "mtime";
pub const PAX_ATIME: &str = "atime";
pub const PAX_CTIME: &str = "ctime"; // Removed from later revision of PAX spec, but was valid
pub const PAX_CHARSET: &str = "charset"; // Currently unused
pub const PAX_COMMENT: &str = "comment"; // Currently unused

pub const PAX_SCHILYXATTR: &str = "SCHILY.xattr.";

// Keywords for GNU sparse files in a PAX extended header.
pub const PAX_GNUSPARSE: &str = "GNU.sparse.";
pub const PAX_GNUSPARSENUMBLOCKS: &str = "GNU.sparse.numblocks";
pub const PAX_GNUSPARSEOFFSET: &str = "GNU.sparse.offset";
pub const PAX_GNUSPARSENUMBYTES: &str = "GNU.sparse.numbytes";
pub const PAX_GNUSPARSEMAP: &str = "GNU.sparse.map";
pub const PAX_GNUSPARSENAME: &str = "GNU.sparse.name";
pub const PAX_GNUSPARSEMAJOR: &str = "GNU.sparse.major";
pub const PAX_GNUSPARSEMINOR: &str = "GNU.sparse.minor";
pub const PAX_GNUSPARSESIZE: &str = "GNU.sparse.size";
pub const PAX_GNUSPARSEREALSIZE: &str = "GNU.sparse.realsize";

/// An iterator over the pax extensions in an archive entry.
///
/// This iterator yields structures which can themselves be parsed into
/// key/value pairs.
pub struct PaxExtensions<'entry> {
    data: slice::Split<'entry, u8, fn(&u8) -> bool>,
}

impl<'entry> PaxExtensions<'entry> {
    /// Create new pax extensions iterator from the given entry data.
    pub fn new(a: &'entry [u8]) -> Self {
        fn is_newline(a: &u8) -> bool {
            *a == b'\n'
        }
        PaxExtensions {
            data: a.split(is_newline),
        }
    }
}

/// A key/value pair corresponding to a pax extension.
pub struct PaxExtension<'entry> {
    key: &'entry [u8],
    value: &'entry [u8],
}

pub fn pax_extensions_value(a: &[u8], key: &str) -> Option<u64> {
    for extension in PaxExtensions::new(a) {
        let current_extension = match extension {
            Ok(ext) => ext,
            Err(_) => return None,
        };
        if current_extension.key() != Ok(key) {
            continue;
        }

        let value = match current_extension.value() {
            Ok(value) => value,
            Err(_) => return None,
        };
        let result = match value.parse::<u64>() {
            Ok(result) => result,
            Err(_) => return None,
        };
        return Some(result);
    }
    None
}

impl<'entry> Iterator for PaxExtensions<'entry> {
    type Item = io::Result<PaxExtension<'entry>>;

    fn next(&mut self) -> Option<io::Result<PaxExtension<'entry>>> {
        let line = match self.data.next() {
            Some([]) => return None,
            Some(line) => line,
            None => return None,
        };

        Some(
            line.iter()
                .position(|b| *b == b' ')
                .and_then(|i| {
                    str::from_utf8(&line[..i])
                        .ok()
                        .and_then(|len| len.parse::<usize>().ok().map(|j| (i + 1, j)))
                })
                .and_then(|(kvstart, reported_len)| {
                    if line.len() + 1 == reported_len {
                        line[kvstart..]
                            .iter()
                            .position(|b| *b == b'=')
                            .map(|equals| (kvstart, equals))
                    } else {
                        None
                    }
                })
                .map(|(kvstart, equals)| PaxExtension {
                    key: &line[kvstart..kvstart + equals],
                    value: &line[kvstart + equals + 1..],
                })
                .ok_or_else(|| other("malformed pax extension")),
        )
    }
}

impl<'entry> PaxExtension<'entry> {
    /// Returns the key for this key/value pair parsed as a string.
    ///
    /// May fail if the key isn't actually utf-8.
    pub fn key(&self) -> Result<&'entry str, str::Utf8Error> {
        str::from_utf8(self.key)
    }

    /// Returns the underlying raw bytes for the key of this key/value pair.
    pub fn key_bytes(&self) -> &'entry [u8] {
        self.key
    }

    /// Returns the value for this key/value pair parsed as a string.
    ///
    /// May fail if the value isn't actually utf-8.
    pub fn value(&self) -> Result<&'entry str, str::Utf8Error> {
        str::from_utf8(self.value)
    }

    /// Returns the underlying raw bytes for this value of this key/value pair.
    pub fn value_bytes(&self) -> &'entry [u8] {
        self.value
    }
}

/// Extension trait for `Builder` to append PAX extended headers.
impl<T: Write> crate::Builder<T> {
    /// Append PAX extended headers to the archive.
    ///
    /// Takes in an iterator over the list of headers to add to convert it into a header set formatted.
    ///
    /// Returns io::Error if an error occurs, else it returns ()
    pub fn append_pax_extensions<'key, 'value>(
        &mut self,
        headers: impl IntoIterator<Item = (&'key str, &'value [u8])>,
    ) -> Result<(), io::Error> {
        // Store the headers formatted before write
        let mut data: Vec<u8> = Vec::new();

        // For each key in headers, convert into a sized space and add it to data.
        // This will then be written in the file
        for (key, value) in headers {
            let mut len_len = 1;
            let mut max_len = 10;
            let rest_len = 3 + key.len() + value.len();
            while rest_len + len_len >= max_len {
                len_len += 1;
                max_len *= 10;
            }
            let len = rest_len + len_len;
            write!(&mut data, "{} {}=", len, key)?;
            data.extend_from_slice(value);
            data.push(b'\n');
        }

        // Ignore the header append if it's empty.
        if data.is_empty() {
            return Ok(());
        }

        // Create a header of type XHeader, set the size to the length of the
        // data, set the entry type to XHeader, and set the checksum
        // then append the header and the data to the archive.
        let mut header = crate::Header::new_ustar();
        let data_as_bytes: &[u8] = &data;
        header.set_size(data_as_bytes.len() as u64);
        header.set_entry_type(crate::EntryType::XHeader);
        header.set_cksum();
        self.append(&header, data_as_bytes)
    }
}
