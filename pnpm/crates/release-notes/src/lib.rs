//! Release notes as a pnpr registry stores them: a Markdown note per package
//! version and per publication. This crate holds the parts of pnpm's client
//! that need no network: the content rules a note must meet, reading a
//! version's section out of a `CHANGELOG.md`, the digest a registry records
//! for a note's initial text, the `<name>/-/notes` document, the upgrade
//! range a reader filters it to, and rendering a note for the terminal.
//!
//! See the release-notes RFC:
//! <https://github.com/pnpm/rfcs/blob/057a5cb08f430a1824c082ed525bea13520bb4dd/pnpr/text/0000-release-notes.md>.

pub use changelog_section::find_changelog_section;
pub use content::{MAX_NOTE_BYTES, NoteContentError, normalize_line_endings, validate_note};
pub use digest::note_digest;
pub use display::render_note;
pub use notes_list::{
    NoteEntry, NotesList, PublicationRef, is_in_upgrade_range, is_publication_segment,
};

mod changelog_section;
mod content;
mod digest;
mod display;
mod notes_list;
