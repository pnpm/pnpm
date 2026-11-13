//! Release notes: Markdown a publisher attaches to a package version or to a
//! publication. Notes are not part of any package document, so editing one
//! changes nothing a resolver or a lockfile reads.

use pnpr_error::{RegistryError, Result};
use pnpr_package_name::Ecosystem;
use ssri::{Algorithm, IntegrityOpts};

/// The largest note pnpr accepts, in bytes.
pub const MAX_NOTE_BYTES: usize = 64 * 1024;

/// A stored note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// The current text; `None` once the note was deleted.
    pub text: Option<String>,
    /// When the note last changed, as an ISO 8601 timestamp.
    pub modified: String,
    /// Who last changed it.
    pub principal: Option<String>,
    /// The `sha256-` digest of the text the note was published with, kept
    /// through later edits and deletion. `None` for a note written after its
    /// publication.
    pub initial_digest: Option<String>,
}

/// A version note sent with a publish.
#[derive(Debug, Clone)]
pub struct VersionNote {
    pub ecosystem: Ecosystem,
    /// The canonical package name.
    pub name: String,
    pub version: String,
    pub text: String,
}

/// A change of a note after its publication.
#[derive(Debug, Clone)]
pub struct NoteChange {
    /// The new text, or `None` to delete the note.
    pub text: Option<String>,
    /// Who made the change.
    pub principal: String,
}

/// The notes a publish request carries, stored in the publication's
/// transaction.
#[derive(Debug, Default, Clone)]
pub struct PublishNotes {
    pub publication: Option<String>,
    pub versions: Vec<VersionNote>,
}

/// Check a note against the content rules: at most [`MAX_NOTE_BYTES`], and no
/// control character other than line feed and tab, no carriage return, and
/// no bidirectional override, so a note cannot move a terminal's cursor,
/// recolor it, or hide text from its reader.
///
/// # Errors
///
/// [`RegistryError::BadRequest`] naming the first rule the note breaks.
pub fn validate_note(text: &str) -> Result<()> {
    if text.len() > MAX_NOTE_BYTES {
        return Err(rejected(&format!("a note is at most {MAX_NOTE_BYTES} bytes")));
    }
    match text
        .chars()
        .find(|character| !allowed(*character))
    {
        Some(character) => Err(rejected(&format!(
            "a note must not contain the character U+{:04X}",
            u32::from(character),
        ))),
        None => Ok(()),
    }
}

fn allowed(character: char) -> bool {
    let bidi_override = matches!(character, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}');
    let control = character.is_control() && !matches!(character, '\n' | '\t');
    !bidi_override && !control
}

fn rejected(reason: &str) -> RegistryError {
    RegistryError::BadRequest { reason: reason.to_string() }
}

/// The `sha256-` digest of a note's text.
#[must_use]
pub fn note_digest(text: &str) -> String {
    IntegrityOpts::new()
        .algorithm(Algorithm::Sha256)
        .chain(text)
        .result()
        .to_string()
}

#[cfg(test)]
mod tests;
