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
///   `\n---\n` separator. A leading `---\n` with no following separator
///   (an env-only file with no main document) also yields `None`.
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
        let read = match reader.read(&mut chunk) {
            Ok(read) => read,
            // A signal interrupting the read is transient, and no command
            // should fail over one.
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if read == 0 {
            // A withheld carriage return is then the file's last byte,
            // and no separator ends in one, so it cannot complete one.
            return Ok(None);
        }
        append_normalized(&mut content, &chunk[..read], &mut withheld_carriage_return);
        if byte_order_mark_pending {
            if content.len() < BYTE_ORDER_MARK.len() {
                continue;
            }
            if content.starts_with(BYTE_ORDER_MARK) {
                content.drain(..BYTE_ORDER_MARK.len());
            }
            byte_order_mark_pending = false;
        }
        if content.len() >= YAML_DOCUMENT_START.len()
            && !content.starts_with(YAML_DOCUMENT_START.as_bytes())
        {
            return Ok(None);
        }
        if let Some(separator) = find_document_separator(&content, scan_from) {
            content.truncate(separator);
            content.drain(..YAML_DOCUMENT_START.len());
            return String::from_utf8(content)
                .map(Some)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error));
        }
        // A separator may straddle the chunk boundary, so resume the
        // scan far enough back to catch a partial match.
        scan_from = content
            .len()
            .saturating_sub(YAML_DOCUMENT_SEPARATOR.len() - 1)
            .max(YAML_DOCUMENT_START.len());
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
    while let Some(index) = bytes.iter().position(|byte| *byte == b'\r') {
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
    rest.find(YAML_DOCUMENT_SEPARATOR).map(|idx| &rest[..idx])
}

#[cfg(test)]
mod tests;
