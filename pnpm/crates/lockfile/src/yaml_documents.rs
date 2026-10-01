//! Multi-document YAML helpers for `pnpm-lock.yaml`.
//!
//! Pnpm v11 writes the lockfile as a stream of up to two YAML
//! documents: an optional first document records the package-manager
//! bootstrap (the deps pulled in by `packageManager` / `devEngines`),
//! and the second document is the regular project lockfile. Pacquet
//! only consumes the second document, so this module strips the
//! leading env document before handing the content to serde.
//!
//! Every entry point here normalizes its input first: a lockfile
//! pacquet did not write may carry a UTF-8 BOM or CRLF line endings (a
//! `core.autocrlf` checkout on Windows), and the document markers below
//! would then match nothing.
//!
//! [`read_first_yaml_document`] is the one entry point that does not
//! take the whole file. The env document is a fraction of a percent of a
//! lockfile, so reading it whole to slice 3 KB out of the front would
//! make every command in a project that pins a pnpm version pay for the
//! dependency graph it never looks at.

use std::{
    borrow::Cow,
    io::{self, Read},
};

/// Document-stream marker that ends one YAML document and starts the
/// next.
pub(crate) const YAML_DOCUMENT_SEPARATOR: &str = "\n---\n";

/// Document-stream marker at the very start of a file.
pub(crate) const YAML_DOCUMENT_START: &str = "---\n";

/// UTF-8 byte-order mark, which a lockfile pacquet did not write may
/// carry.
const BYTE_ORDER_MARK: &[u8] = "\u{feff}".as_bytes();

/// How much of the lockfile one read pulls in. An env document fits in
/// the first chunk, so the whole read is normally a single syscall.
const READ_BUFFER_SIZE: usize = 64 * 1024;

/// Strip a leading UTF-8 BOM and rewrite CRLF as LF, so the rest of the
/// lockfile machinery only ever sees the byte shape pacquet writes.
#[must_use]
pub fn normalize_lockfile_content(content: &str) -> Cow<'_, str> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    if content.contains("\r\n") {
        Cow::Owned(content.replace("\r\n", "\n"))
    } else {
        Cow::Borrowed(content)
    }
}

/// Extract the main lockfile document (second YAML document) from a
/// combined file.
#[must_use]
pub fn extract_main_document(content: &str) -> Cow<'_, str> {
    match normalize_lockfile_content(content) {
        Cow::Borrowed(content) => Cow::Borrowed(main_document_of(content)),
        Cow::Owned(content) => Cow::Owned(main_document_of(&content).to_string()),
    }
}

/// Extract the env lockfile document (first YAML document) from a
/// combined file:
///
/// - The file must begin with `---\n`; otherwise it carries no env
///   document and this returns `None`.
/// - Returns the slice between the leading `---\n` and the next
///   `\n---\n` separator.
/// - A leading `---\n` with no following separator is an env-only file:
///   a lockfile with no dependencies whose empty main document was
///   trimmed off together with the separator. Its env document is
///   everything after the leading marker, less a closing `---` line, as
///   long as its root importer opens with `configDependencies`.
#[must_use]
pub fn extract_env_document(content: &str) -> Option<Cow<'_, str>> {
    match normalize_lockfile_content(content) {
        Cow::Borrowed(content) => env_document_of(content).map(Cow::Borrowed),
        Cow::Owned(content) => env_document_of(&content).map(|doc| Cow::Owned(doc.to_string())),
    }
}

/// Read the env lockfile document (first YAML document) out of a
/// lockfile without reading past it, applying the same rules as
/// [`extract_env_document`].
///
/// The reader is consumed in chunks and abandoned as soon as the answer
/// is known: after 4 bytes for a lockfile that carries no env document,
/// after the separator that closes the env document otherwise.
pub(crate) fn read_first_yaml_document(reader: impl Read) -> io::Result<Option<String>> {
    read_first_yaml_document_in_chunks(reader, READ_BUFFER_SIZE)
}

fn read_first_yaml_document_in_chunks(
    mut reader: impl Read,
    chunk_size: usize,
) -> io::Result<Option<String>> {
    let mut chunk = vec![0; chunk_size];
    let mut content = Vec::new();
    let mut withheld_carriage_return = false;
    let mut byte_order_mark_pending = true;
    let mut scan_from = YAML_DOCUMENT_START.len();
    loop {
        let read = read_chunk(&mut reader, &mut chunk)?;
        if read == 0 {
            return document_without_separator(content, withheld_carriage_return);
        }
        append_normalized(&mut content, &chunk[..read], &mut withheld_carriage_return);
        if !take_byte_order_mark(&mut content, &mut byte_order_mark_pending) {
            continue;
        }
        if starts_another_document(&content) {
            return Ok(None);
        }
        if let Some(separator) = find_document_separator(&content, scan_from) {
            return first_document(content, separator).map(Some);
        }
        // A separator may straddle the chunk boundary, so resume the
        // scan far enough back to catch a partial match.
        scan_from = content
            .len()
            .saturating_sub(YAML_DOCUMENT_SEPARATOR.len() - 1)
            .max(YAML_DOCUMENT_START.len());
    }
}

/// The env document of a file read to its end without finding a separator,
/// when it opens with the start marker. A withheld carriage return is the
/// file's last byte.
fn document_without_separator(
    mut content: Vec<u8>,
    withheld_carriage_return: bool,
) -> io::Result<Option<String>> {
    if withheld_carriage_return {
        content.push(b'\r');
    }
    if !content.starts_with(YAML_DOCUMENT_START.as_bytes()) {
        return Ok(None);
    }
    let content = String::from_utf8(content)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok(env_only_document(&content[YAML_DOCUMENT_START.len()..]).map(str::to_string))
}

/// The document body between the start marker and the separator that closes
/// it.
fn first_document(mut content: Vec<u8>, separator: usize) -> io::Result<String> {
    content.truncate(separator);
    content.drain(..YAML_DOCUMENT_START.len());
    String::from_utf8(content).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// Strip a leading byte-order mark once enough bytes have arrived to tell
/// whether there is one. Reports whether the caller can go on reading this
/// chunk, or has to pull more bytes first.
fn take_byte_order_mark(content: &mut Vec<u8>, pending: &mut bool) -> bool {
    if !*pending {
        return true;
    }
    if content.len() < BYTE_ORDER_MARK.len() {
        return false;
    }
    *pending = false;
    if content.starts_with(BYTE_ORDER_MARK) {
        content.drain(..BYTE_ORDER_MARK.len());
    }
    true
}

/// Whether enough has been read to tell that the file does not open with the
/// document-start marker this reader expects.
fn starts_another_document(content: &[u8]) -> bool {
    content.len() >= YAML_DOCUMENT_START.len()
        && !content.starts_with(YAML_DOCUMENT_START.as_bytes())
}

/// Read one chunk, retrying an interrupted read: a signal is transient, and
/// no command should fail over one.
fn read_chunk(reader: &mut impl Read, chunk: &mut [u8]) -> io::Result<usize> {
    loop {
        match reader.read(chunk) {
            Ok(read) => return Ok(read),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
}

/// Append `bytes` to `content`, rewriting CRLF as LF. A carriage return
/// ending the chunk is withheld until the next chunk says whether an LF
/// follows it.
fn append_normalized(content: &mut Vec<u8>, mut bytes: &[u8], withheld_carriage_return: &mut bool) {
    if *withheld_carriage_return {
        *withheld_carriage_return = false;
        if let [b'\n', rest @ ..] = bytes {
            content.push(b'\n');
            bytes = rest;
        } else {
            content.push(b'\r');
        }
    }
    if let [rest @ .., b'\r'] = bytes {
        *withheld_carriage_return = true;
        bytes = rest;
    }
    while let Some(index) = bytes
        .iter()
        .position(|byte| *byte == b'\r')
    {
        let (head, tail) = bytes.split_at(index);
        content.extend_from_slice(head);
        if tail.get(1) == Some(&b'\n') {
            content.push(b'\n');
            bytes = &tail[2..];
        } else {
            content.push(b'\r');
            bytes = &tail[1..];
        }
    }
    content.extend_from_slice(bytes);
}

fn find_document_separator(content: &[u8], scan_from: usize) -> Option<usize> {
    let separator = YAML_DOCUMENT_SEPARATOR.as_bytes();
    content
        .get(scan_from..)?
        .windows(separator.len())
        .position(|window| window == separator)
        .map(|index| scan_from + index)
}

fn main_document_of(content: &str) -> &str {
    let Some(rest) = content.strip_prefix(YAML_DOCUMENT_START) else {
        return content;
    };
    match rest.find(YAML_DOCUMENT_SEPARATOR) {
        Some(idx) => &rest[idx + YAML_DOCUMENT_SEPARATOR.len()..],
        None => "",
    }
}

fn env_document_of(content: &str) -> Option<&str> {
    let rest = content.strip_prefix(YAML_DOCUMENT_START)?;
    match rest.find(YAML_DOCUMENT_SEPARATOR) {
        Some(idx) => Some(&rest[..idx]),
        None => env_only_document(rest),
    }
}

/// The env document of a file that has no main document after it: `rest`
/// up to where the separator would start, so a closing `---` line that has
/// no newline after it is dropped too. Only a body whose root importer
/// opens with the `configDependencies` key every env document writes
/// qualifies, so a main lockfile that merely opens with `---` is not
/// mistaken for one.
fn env_only_document(rest: &str) -> Option<&str> {
    let document = rest
        .strip_suffix("\n---")
        .or_else(|| rest.strip_suffix('\n'))
        .unwrap_or(rest);
    document.contains(ROOT_IMPORTER_CONFIG_DEPENDENCIES).then_some(document)
}

/// How an env document lays out the start of its root importer.
const ROOT_IMPORTER_CONFIG_DEPENDENCIES: &str = "\n  .:\n    configDependencies:";

#[cfg(test)]
mod tests;
