//! Recognizing a next-format document.

use crate::{
    ComVer,
    json_document::{is_json_document, json_lockfile_version},
};

/// Which shape a lockfile document's sections have.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shape {
    /// The shape its `lockfileVersion` names.
    OfVersion,
    /// Peer-suffixed, whatever its `lockfileVersion`: the current lockfile,
    /// which records the wanted lockfile's version but is always written
    /// suffixed, so that reading it needs no conversion.
    Suffixed,
}

/// Whether the document's `lockfileVersion` is the next format's. The
/// syntax does not say: JSON is YAML, and a 9.0 lockfile can be written as
/// JSON too. pnpm writes the field first, so this reads little of the
/// document.
pub(crate) fn declares_next_format(document: &str) -> bool {
    let version = if is_json_document(document) {
        json_lockfile_version(document)
    } else {
        document.lines().find_map(yaml_version_value)
    };
    version
        .and_then(|value| value.parse::<ComVer>().ok())
        .is_some_and(|version| version.major == crate::NEXT_LOCKFILE_MAJOR)
}

/// The value of a top-level `lockfileVersion:` line, with the key quoted or
/// not and a trailing comment dropped; `None` for any other line.
fn yaml_version_value(line: &str) -> Option<&str> {
    let value = ["lockfileVersion:", "'lockfileVersion':", r#""lockfileVersion":"#]
        .iter()
        .find_map(|key| line.strip_prefix(key))?;
    let value = value.split_once(" #").map_or(value, |(before, _)| before);
    Some(value.trim().trim_matches(['\'', '"']))
}
