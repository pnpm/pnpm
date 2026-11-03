use derive_more::{Display, Error};
use miette::Diagnostic;

/// The largest note, in UTF-8 bytes, that a pnpr registry accepts.
pub const MAX_NOTE_BYTES: usize = 64 * 1024;

/// Why a registry would reject a note.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum NoteContentError {
    #[display(
        "The release notes are {size} bytes long, more than the {MAX_NOTE_BYTES} bytes a registry accepts"
    )]
    #[diagnostic(code(ERR_PNPM_RELEASE_NOTES_TOO_LARGE))]
    TooLarge {
        #[error(not(source))]
        size: usize,
    },

    #[display("The release notes contain the character U+{code_point:04X} on line {line}")]
    #[diagnostic(
        code(ERR_PNPM_RELEASE_NOTES_FORBIDDEN_CHARACTER),
        help(
            "Release notes may not contain control characters other than line feed and tab, or bidirectional override characters."
        )
    )]
    ForbiddenCharacter {
        #[error(not(source))]
        code_point: u32,
        line: usize,
    },
}

/// `text` with every CRLF and lone CR turned into LF, the line ending a
/// registry accepts.
#[must_use]
pub fn normalize_line_endings(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Check `note` against the content rules of a registry that stores release
/// notes, so a publish fails before anything is uploaded. Line endings are
/// expected to be normalized already: a carriage return is rejected.
pub fn validate_note(note: &str) -> Result<(), NoteContentError> {
    if note.len() > MAX_NOTE_BYTES {
        return Err(NoteContentError::TooLarge { size: note.len() });
    }
    for (index, line) in note.split('\n').enumerate() {
        if let Some(character) = line.chars().find(|&character| is_forbidden(character)) {
            return Err(NoteContentError::ForbiddenCharacter {
                code_point: u32::from(character),
                line: index + 1,
            });
        }
    }
    Ok(())
}

fn is_forbidden(character: char) -> bool {
    (character.is_control() && character != '\t') || is_bidi_override(character)
}

/// The embedding, override, and isolate controls that reorder the text
/// around them.
fn is_bidi_override(character: char) -> bool {
    matches!(character, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

#[cfg(test)]
mod tests;
