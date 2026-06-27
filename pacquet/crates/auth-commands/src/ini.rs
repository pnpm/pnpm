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
                Some((key.trim().to_string(), value.trim().to_string()))
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
    pub fn serialize(&self) -> String {
        use std::fmt::Write;
        self.entries.iter().fold(String::new(), |mut out, (key, value)| {
            writeln!(out, "{key}={value}").expect("writing to a String never fails");
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

#[cfg(test)]
mod tests;
