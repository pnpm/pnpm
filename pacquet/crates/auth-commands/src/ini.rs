//! A flat `key=value` INI reader/writer for `auth.ini`.
//!
//! `pnpm login` / `pnpm logout` keep their tokens in `auth.ini`, a file
//! that only ever holds top-level `//host/path/:_authToken=<token>`
//! lines (no sections, no arrays). Upstream pnpm round-trips it through
//! `read-ini-file` / `write-ini-file`; pacquet has no INI crate, and the
//! `.npmrc` parser in `pacquet-config` is likewise hand-rolled. This is
//! the matching minimal reader/writer for the auth-command file.
//!
//! Entries keep their on-disk order so removing one token rewrites the
//! file without churning the rest. Section headers (`[name]`), bare keys
//! (no `=`), and comment lines (`;` / `#`) are not part of `auth.ini`'s
//! shape and are skipped on read.
//!
//! A value that would break the flat one-line shape — one containing `=`,
//! CR, or LF — is written as a JSON string, matching the `ini` package
//! `write-ini-file` uses, and decoded back on read. Without this a
//! registry-controlled auth token with an embedded newline could plant
//! extra entries in `auth.ini`.

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
                let (key, value) = line.split_once('=')?;
                Some((key.trim().to_string(), decode_value(value.trim())))
            })
            .collect();
        IniSettings { entries }
    }

    /// Set `key` to `value`, updating the first existing entry in place
    /// (preserving its position) and dropping any later duplicates so the
    /// key resolves to a single value, or appending when the key is absent.
    /// Mirrors assigning a property on the object `write-ini-file`
    /// serializes (and the all-duplicates handling of [`remove`](Self::remove)).
    pub fn set(&mut self, key: &str, value: &str) {
        let mut updated = false;
        self.entries.retain_mut(|(entry_key, entry_value)| {
            if entry_key.as_str() != key {
                return true;
            }
            if updated {
                return false;
            }
            *entry_value = value.to_string();
            updated = true;
            true
        });
        if !updated {
            self.entries.push((key.to_string(), value.to_string()));
        }
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
            writeln!(out, "{key}={}", encode_value(value))
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

/// Quote a value that would otherwise be misread on the way back, as a JSON
/// string — matching the `ini` package `write-ini-file` uses so
/// [`encode_value`] and [`decode_value`] stay inverses. Quoting is required
/// when the value:
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

#[cfg(test)]
mod tests;
