//! Line-preserving flat-key INI read/modify/write for `.npmrc` and `auth.ini`.
//!
//! `pnpm config set` and `pnpm config delete` modify these configuration files.
//! To avoid destroying comments (`;` / `#`), blank lines, or repeated keys
//! (such as multiple `ca=` certificate authority lines in `.npmrc`), this module
//! parses the INI file into a document model that preserves untouched lines verbatim
//! while allowing in-place replacement or removal of targeted keys.

use std::{fs, io, path::Path};

/// The line ending for a line in an INI file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    /// Unix line ending (`\n`).
    Lf,
    /// Windows line ending (`\r\n`).
    CrLf,
    /// No trailing line ending (e.g. final line without a trailing newline).
    None,
}

impl LineEnding {
    /// Return the string representation of this line ending.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
            Self::None => "",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LineKind {
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

/// A line in an INI file (.npmrc or auth.ini).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IniLine {
    kind: LineKind,
    terminator: LineEnding,
}

impl IniLine {
    fn key(&self) -> Option<&str> {
        match &self.kind {
            LineKind::Entry { key, .. } => Some(key.as_str()),
            LineKind::Raw(_) => None,
        }
    }

    fn matches_key(&self, target_key: &str) -> bool {
        self.key().is_some_and(|line_key| keys_match(line_key, target_key))
    }

    #[cfg(test)]
    fn value(&self) -> Option<&str> {
        match &self.kind {
            LineKind::Entry { value, .. } => Some(value.as_str()),
            LineKind::Raw(_) => None,
        }
    }
}

fn keys_match(key_a: &str, key_b: &str) -> bool {
    let normalized_a = key_a.strip_suffix("[]").unwrap_or(key_a);
    let normalized_b = key_b.strip_suffix("[]").unwrap_or(key_b);
    normalized_a == normalized_b
}

/// A parsed INI document preserving comments, blank lines, repeated keys, and line terminators.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IniDocument {
    lines: Vec<IniLine>,
    default_ending: LineEnding,
    has_bom: bool,
}

impl Default for IniDocument {
    fn default() -> Self {
        Self { lines: Vec::new(), default_ending: LineEnding::Lf, has_bom: false }
    }
}

impl IniDocument {
    /// Parse `text` into an [`IniDocument`], recording every line and
    /// preserving comments, blank lines, repeated keys, and line terminators.
    pub fn parse(text: &str) -> Self {
        let (has_bom, text_without_bom) = match text.strip_prefix('\u{feff}') {
            Some(stripped) => (true, stripped),
            None => (false, text),
        };
        let default_ending =
            if text_without_bom.contains("\r\n") { LineEnding::CrLf } else { LineEnding::Lf };
        let lines = split_line_terminators(text_without_bom)
            .into_iter()
            .map(|(content, terminator)| IniLine { kind: parse_line_kind(content), terminator })
            .collect();

        Self { lines, default_ending, has_bom }
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
            match &line.kind {
                LineKind::Raw(raw) => {
                    output.push_str(raw);
                }
                LineKind::Entry { key, value, raw } => {
                    if let Some(raw) = raw {
                        output.push_str(raw);
                    } else {
                        output.push_str(key);
                        output.push('=');
                        output.push_str(value);
                    }
                }
            }
            output.push_str(line.terminator.as_str());
        }
        output
    }

    /// Return the last value for `key`, if present.
    #[cfg(test)]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.lines
            .iter()
            .rev()
            .find_map(|line| if line.matches_key(key) { line.value() } else { None })
    }

    /// Return all values for `key` in document order.
    #[cfg(test)]
    pub fn get_all(&self, key: &str) -> Vec<&str> {
        self.lines
            .iter()
            .filter_map(|line| if line.matches_key(key) { line.value() } else { None })
            .collect()
    }

    /// Remove all entries matching `key`. Returns `true` if at least one entry was removed.
    pub fn delete(&mut self, key: &str) -> bool {
        let before = self.lines.len();
        self.lines.retain(|line| !line.matches_key(key));
        self.lines.len() != before
    }

    fn replace_existing_key(
        &mut self,
        key: &str,
        values: &[String],
        first_index: usize,
        is_array: bool,
    ) {
        let old_terminator = self.lines[first_index].terminator;
        let existing_key = self.lines[first_index].key().unwrap_or(key);
        let write_key = resolve_replace_key(key, existing_key, is_array);

        let new_entries: Vec<IniLine> = values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let terminator =
                    if index + 1 == values.len() { old_terminator } else { self.default_ending };
                IniLine {
                    kind: LineKind::Entry {
                        key: write_key.clone(),
                        value: value.clone(),
                        raw: None,
                    },
                    terminator,
                }
            })
            .collect();

        self.lines.splice(first_index..=first_index, new_entries);

        let mut index = first_index + values.len();
        while index < self.lines.len() {
            if self.lines[index].matches_key(key) {
                self.lines.remove(index);
            } else {
                index += 1;
            }
        }
    }

    fn append_new_key(&mut self, key: &str, values: &[String], is_array: bool) {
        let had_no_trailing_newline = if let Some(last_line) = self.lines.last_mut()
            && last_line.terminator == LineEnding::None
        {
            last_line.terminator = self.default_ending;
            true
        } else {
            false
        };
        let write_key = resolve_append_key(key, is_array);
        for (index, value) in values.iter().enumerate() {
            let is_last = index + 1 == values.len();
            let terminator = if is_last && had_no_trailing_newline {
                LineEnding::None
            } else {
                self.default_ending
            };
            self.lines.push(IniLine {
                kind: LineKind::Entry { key: write_key.clone(), value: value.clone(), raw: None },
                terminator,
            });
        }
    }

    /// Set `key` to `values`.
    ///
    /// - If `values` is empty, all entries matching `key` are removed.
    /// - If `key` already exists, the first occurrence is replaced in-place
    ///   with the new value(s), and any subsequent occurrences of `key` are removed.
    /// - If `key` does not exist, the new entry (or entries) are appended at the end.
    pub fn set(&mut self, key: &str, values: &[String]) {
        let is_array = values.len() > 1 || key.ends_with("[]");
        self.set_with_kind(key, values, is_array);
    }

    /// Set `key` explicitly as an array of values.
    pub fn set_array(&mut self, key: &str, values: &[String]) {
        self.set_with_kind(key, values, true);
    }

    fn set_with_kind(&mut self, key: &str, values: &[String], is_array: bool) {
        if values.is_empty() {
            self.delete(key);
            return;
        }

        let first_index = self.lines
            .iter()
            .position(|line| line.matches_key(key));

        if let Some(index) = first_index {
            self.replace_existing_key(key, values, index, is_array);
        } else {
            self.append_new_key(key, values, is_array);
        }
    }

    /// Iterate over key-value entries in document order.
    #[cfg(test)]
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.lines
            .iter()
            .filter_map(|line| match &line.kind {
                LineKind::Entry { key, value, .. } => Some((key.as_str(), value.as_str())),
                LineKind::Raw(_) => None,
            })
    }
}

fn split_line_terminators(mut text: &str) -> Vec<(&str, LineEnding)> {
    let mut lines = Vec::new();
    while !text.is_empty() {
        if let Some(pos) = text.find('\n') {
            let (content, terminator) = if pos > 0 && text.as_bytes()[pos - 1] == b'\r' {
                (&text[..pos - 1], LineEnding::CrLf)
            } else {
                (&text[..pos], LineEnding::Lf)
            };
            lines.push((content, terminator));
            text = &text[pos + 1..];
        } else {
            lines.push((text, LineEnding::None));
            break;
        }
    }
    lines
}

fn parse_line_kind(content: &str) -> LineKind {
    let trimmed = content.trim();
    if trimmed.is_empty() || trimmed.starts_with([';', '#']) {
        return LineKind::Raw(content.to_string());
    }
    if let Some((raw_key, raw_value)) = content.split_once('=') {
        let key = raw_key.trim();
        if !key.is_empty() {
            return LineKind::Entry {
                key: key.to_string(),
                value: raw_value.trim().to_string(),
                raw: Some(content.to_string()),
            };
        }
    }
    LineKind::Raw(content.to_string())
}

fn is_ca_key(key: &str) -> bool {
    key.strip_suffix("[]").unwrap_or(key) == "ca"
}

fn resolve_replace_key(key: &str, existing_key: &str, is_array: bool) -> String {
    if is_ca_key(key) {
        "ca".to_string()
    } else if key.ends_with("[]") {
        key.to_string()
    } else if is_array && existing_key.ends_with("[]") {
        existing_key.to_string()
    } else {
        key.to_string()
    }
}

fn resolve_append_key(key: &str, is_array: bool) -> String {
    if is_ca_key(key) {
        "ca".to_string()
    } else if is_array && !key.ends_with("[]") {
        format!("{key}[]")
    } else {
        key.to_string()
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
