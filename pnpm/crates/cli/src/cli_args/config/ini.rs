//! Line-preserving flat-key INI read/modify/write for `.npmrc` and `auth.ini`.
//!
//! `pnpm config set` and `pnpm config delete` modify these configuration files.
//! To avoid destroying comments (`;` / `#`), blank lines, or repeated keys
//! (such as multiple `ca=` certificate authority lines in `.npmrc`), this module
//! parses the INI file into a document model that preserves untouched lines verbatim
//! while allowing in-place replacement or removal of targeted keys.

use std::{fs, io, path::Path};

/// A line in an INI file (.npmrc or auth.ini).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IniLine {
    /// A comment line, empty line, or unrecognized line, preserved verbatim.
    Raw(String),
    /// A `key=value` configuration entry.
    Entry {
        key: String,
        value: String,
        /// The original line text, preserved verbatim if value and key were unchanged.
        raw: Option<String>,
    },
}

/// A parsed INI document preserving comments, blank lines, and repeated keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IniDocument {
    lines: Vec<IniLine>,
    newline: &'static str,
    has_bom: bool,
}

impl Default for IniDocument {
    fn default() -> Self {
        Self { lines: Vec::new(), newline: "\n", has_bom: false }
    }
}

impl IniDocument {
    /// Parse `text` into an [`IniDocument`], recording every line and
    /// preserving comments, blank lines, and repeated keys.
    pub fn parse(text: &str) -> Self {
        let (has_bom, text_without_bom) = match text.strip_prefix('\u{feff}') {
            Some(stripped) => (true, stripped),
            None => (false, text),
        };
        let newline = if text_without_bom.contains("\r\n") { "\r\n" } else { "\n" };
        let lines = text_without_bom
            .lines()
            .map(Self::parse_line)
            .collect();

        Self { lines, newline, has_bom }
    }

    fn parse_line(line: &str) -> IniLine {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with([';', '#']) {
            return IniLine::Raw(line.to_string());
        }
        if let Some((raw_key, raw_value)) = line.split_once('=') {
            let key = raw_key.trim();
            if !key.is_empty() {
                return IniLine::Entry {
                    key: key.to_string(),
                    value: raw_value.trim().to_string(),
                    raw: Some(line.to_string()),
                };
            }
        }
        IniLine::Raw(line.to_string())
    }

    /// Read `path` into an [`IniDocument`]. A missing file produces an empty document;
    /// any other read error propagates.
    pub fn read(path: &Path) -> io::Result<Self> {
        match fs::read_to_string(path) {
            Ok(text) => Ok(Self::parse(&text)),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(err),
        }
    }

    /// Write document contents to `path`, creating parent directories
    /// and replacing the file atomically and symlink-safely (see [`pnpm_fs::write_atomic`]).
    pub fn write(&self, path: &Path) -> io::Result<()> {
        let contents = self.serialize();
        pnpm_fs::write_atomic(path, contents.as_bytes())
    }

    /// Serialize the document back to a string.
    pub fn serialize(&self) -> String {
        if self.lines.is_empty() {
            return String::new();
        }
        let mut output = String::new();
        if self.has_bom {
            output.push('\u{feff}');
        }
        for line in &self.lines {
            match line {
                IniLine::Raw(raw) => {
                    output.push_str(raw);
                }
                IniLine::Entry { key, value, raw } => {
                    if let Some(raw) = raw {
                        output.push_str(raw);
                    } else {
                        output.push_str(key);
                        output.push('=');
                        output.push_str(value);
                    }
                }
            }
            output.push_str(self.newline);
        }
        output
    }

    /// Return the last value for `key`, if present.
    #[cfg(test)]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.lines
            .iter()
            .rev()
            .find_map(|line| match line {
                IniLine::Entry { key: k, value, .. } if k == key => Some(value.as_str()),
                _ => None,
            })
    }

    /// Return all values for `key` in document order.
    #[cfg(test)]
    pub fn get_all(&self, key: &str) -> Vec<&str> {
        self.lines
            .iter()
            .filter_map(|line| match line {
                IniLine::Entry { key: k, value, .. } if k == key => Some(value.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Remove all entries matching `key`. Returns `true` if at least one entry was removed.
    pub fn delete(&mut self, key: &str) -> bool {
        let before = self.lines.len();
        self.lines.retain(|line| match line {
            IniLine::Entry { key: k, .. } => k != key,
            IniLine::Raw(_) => true,
        });
        self.lines.len() != before
    }

    fn replace_existing_key(&mut self, key: &str, values: &[String], first_index: usize) {
        let new_entries: Vec<IniLine> = values
            .iter()
            .map(|val| IniLine::Entry { key: key.to_string(), value: val.clone(), raw: None })
            .collect();

        self.lines.splice(first_index..=first_index, new_entries);

        let mut index = first_index + values.len();
        while index < self.lines.len() {
            if matches!(&self.lines[index], IniLine::Entry { key: k, .. } if k == key) {
                self.lines.remove(index);
            } else {
                index += 1;
            }
        }
    }

    /// Set `key` to `values`.
    ///
    /// - If `values` is empty, all entries matching `key` are removed.
    /// - If `key` already exists, the first occurrence is replaced in-place
    ///   with the new value(s), and any subsequent occurrences of `key` are removed.
    /// - If `key` does not exist, the new entry (or entries) are appended at the end.
    pub fn set(&mut self, key: &str, values: &[String]) {
        if values.is_empty() {
            self.delete(key);
            return;
        }

        let first_index = self.lines
            .iter()
            .position(|line| match line {
                IniLine::Entry { key: k, .. } => k == key,
                IniLine::Raw(_) => false,
            });

        if let Some(index) = first_index {
            self.replace_existing_key(key, values, index);
        } else {
            for val in values {
                self.lines.push(IniLine::Entry {
                    key: key.to_string(),
                    value: val.clone(),
                    raw: None,
                });
            }
        }
    }

    /// Iterate over key-value entries in document order.
    #[cfg(test)]
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.lines
            .iter()
            .filter_map(|line| match line {
                IniLine::Entry { key, value, .. } => Some((key.as_str(), value.as_str())),
                IniLine::Raw(_) => None,
            })
    }
}

/// Read `path` into an [`IniDocument`]. A missing file produces an empty document;
/// any other read error propagates.
pub fn read(path: &Path) -> io::Result<IniDocument> {
    IniDocument::read(path)
}

/// Write `doc` to `path`.
pub fn write(path: &Path, doc: &IniDocument) -> io::Result<()> {
    doc.write(path)
}

#[cfg(test)]
mod tests;
