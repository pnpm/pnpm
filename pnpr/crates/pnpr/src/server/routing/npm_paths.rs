use serde::Deserialize;

// Path shapes. Each names only the segments its handlers read: the
// `/~<name>/` registration captures a `registry` segment too, and the
// `-rev` routes also capture the document revision.

/// A package addressed by a single segment: an unscoped name, or a scoped name
/// percent-encoded as `@scope%2Fname`.
#[derive(Deserialize)]
pub(super) struct NamePath {
    pub(super) name: String,
}

#[derive(Deserialize)]
pub(super) struct DeletePath {
    pub(super) name: String,
    pub(super) rev: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct RevisionPath {
    pub(super) name: String,
    pub(super) rev: String,
}

/// npm's overloaded two-segment address. Which package it names, and which
/// resource of it, depends on the first segment's shape and on the method, so
/// neither segment can be given a resource name here.
#[derive(Deserialize)]
pub(super) struct TwoSegments {
    pub(super) first: String,
    pub(super) second: String,
}

#[derive(Deserialize)]
pub(super) struct ScopedVersionPath {
    pub(super) scope: String,
    pub(super) name: String,
    pub(super) version: String,
}

#[derive(Deserialize)]
pub(super) struct TarballPath {
    pub(super) name: String,
    pub(super) filename: String,
}

#[derive(Deserialize)]
pub(super) struct ScopedTarballPath {
    pub(super) scope: String,
    pub(super) name: String,
    pub(super) filename: String,
}

#[derive(Deserialize)]
pub(super) struct ScopedRevPath {
    pub(super) scope: String,
    pub(super) name: String,
    pub(super) rev: String,
}

#[derive(Deserialize)]
pub(super) struct DistTagPath {
    pub(super) name: String,
    pub(super) tag: String,
}
