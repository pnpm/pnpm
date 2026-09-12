//! A flat `key=value` INI reader/writer for `auth.ini`.
//!
//! `pnpm login` writes to the global `config.yaml`, but versions before it
//! kept their tokens in `auth.ini`, a file that only ever holds top-level
//! `//host/path/:_authToken=<token>` lines (no sections, no arrays). This
//! reader/writer is what lets `pnpm logout` take a token out of one without
//! disturbing the entries around it. Upstream pnpm round-trips the file
//! through `read-ini-file` / `write-ini-file`; pacquet has no INI crate, and
//! the `.npmrc` parser in `pnpm-config` is likewise hand-rolled.
//!
//! Entries keep their on-disk order so removing one token rewrites the
//! file without churning the rest. Section headers (`[name]`), bare keys
//! (no `=`), and comment lines (`;` / `#`) are not part of `auth.ini`'s
//! shape and are skipped on read.
//!
//! A key or value that would be misread on the way back — one containing
//! `=`, CR, or LF, one already `"`-wrapped, one padded with whitespace, or
//! one starting with `[` — is written as a JSON string (the same quoting
//! the `ini` package's `write-ini-file` applies) and decoded on read, so
//! that removing one entry leaves every other entry saying what it said.
//!
//! The `ini` package additionally backslash-escapes inline `;` / `#` (which
//! it reads as comment starts) and unwraps single-quoted values. That is
//! deliberately omitted here: the opaque bearer tokens this file holds are
//! `[A-Za-z0-9+/=._-]` and never contain `;`, `#`, or quotes, and pacquet
//! only skips comments at line start, so such values round-trip regardless.

use std::borrow::Cow;

/// An ordered set of `auth.ini` entries.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct IniSettings {
    entries: Vec<(String, String)>,
}

impl IniSettings {
    /// Parse flat `key=value` lines, preserving order and skipping
    /// blank lines, comments, section headers, and bare keys.
    pub fn parse(text: &str) -> Self {
        let entries = text
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                if line.is_empty() || line.starts_with([';', '#', '[']) {
                    return None;
                }
                let (key, value) = split_key_value(line)?;
                Some((decode_value(key.trim()), decode_value(value.trim())))
            })
            .collect();
        IniSettings { entries }
    }

    /// Remove every entry whose key equals `key`. Returns `true` when at
    /// least one entry was removed.
    pub fn remove(&mut self, key: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|(entry_key, _)| entry_key != key);
        self.entries.len() != before
    }

    /// Render back to flat `key=value` lines, each terminated by `\n`.
    /// Values that would break the one-line shape are JSON-quoted (see
    /// [`encode_value`]).
    pub fn serialize(&self) -> String {
        use std::fmt::Write;
        self.entries.iter().fold(String::new(), |mut out, (key, value)| {
            writeln!(out, "{}={}", encode_value(key), encode_value(value))
                .expect("writing to a String never fails");
            out
        })
    }

    #[cfg(test)]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find_map(|(entry_key, value)| (entry_key == key).then_some(value.as_str()))
    }
}

/// Quote a key or value that would otherwise be misread on the way back, as a
/// JSON string — the same quoting the `ini` package's `write-ini-file`
/// applies, so [`encode_value`] and [`decode_value`] stay inverses (`ini`'s
/// inline `;` / `#` escaping is out of scope; see the module docs). Quoting is
/// required when the string:
///
/// - contains `=`, CR, or LF (a registry-controlled token with an embedded
///   newline would otherwise plant extra `auth.ini` entries);
/// - is already `"`-wrapped, so [`decode_value`] would strip its quotes;
/// - has leading/trailing whitespace, which [`parse`](IniSettings::parse) trims;
/// - starts with `[`, which reads as a section header.
fn encode_value(value: &str) -> Cow<'_, str> {
    let needs_quoting = value.contains(['=', '\r', '\n'])
        || value.starts_with('[')
        || value != value.trim()
        || (value.len() > 1 && value.starts_with('"') && value.ends_with('"'));
    if needs_quoting {
        serde_json::to_string(value).expect("serializing a string never fails").into()
    } else {
        Cow::Borrowed(value)
    }
}

/// Reverse of [`encode_value`]: a JSON-quoted value is decoded to its literal
/// contents; anything else is taken verbatim. Mirrors the `ini` package's
/// quoted-value handling on read.
fn decode_value(value: &str) -> String {
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        serde_json::from_str::<String>(value).unwrap_or_else(|_| value.to_owned())
    } else {
        value.to_owned()
    }
}

/// Split a stored line into its still-encoded key and value halves. A
/// JSON-quoted key can itself contain `=`, so the separator is the `=` after
/// the key's closing quote; an unquoted key never contains `=` (it would have
/// been quoted by [`encode_value`]), so its first `=` is the separator.
fn split_key_value(line: &str) -> Option<(&str, &str)> {
    if line.starts_with('"') {
        let close = closing_quote_index(line)?;
        let (key, rest) = line.split_at(close + 1);
        Some((key, rest.trim_start().strip_prefix('=')?))
    } else {
        line.split_once('=')
    }
}

/// Byte index of the closing `"` of the JSON string starting at index 0,
/// skipping `\`-escaped bytes. `None` for an unterminated string. Byte
/// scanning is safe: a UTF-8 continuation byte is never `"` (`0x22`) or `\`
/// (`0x5c`).
fn closing_quote_index(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut index = 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'"' => return Some(index),
            _ => index += 1,
        }
    }
    None
}

#[cfg(test)]
mod tests;
