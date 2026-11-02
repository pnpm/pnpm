//! The types of an atomic publication: the documents a request commits
//! together, the publication that records what it introduced, and the
//! recorded outcome a retry of the request gets back.

use super::Generation;
use crate::HostedDocumentVersion;
use pnpr_package_name::Ecosystem;

/// One package document of a publication.
#[derive(Debug, Clone)]
pub struct BatchDocument {
    pub ecosystem: Ecosystem,
    /// The canonical package name.
    pub name: String,
    pub bytes: Vec<u8>,
    /// The stored document `bytes` was computed from; `None` when there was
    /// none.
    pub expected: Option<HostedDocumentVersion>,
}

/// An artifact a publication introduced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactMember {
    pub ecosystem: Ecosystem,
    pub name: String,
    /// The version the artifact belongs to; empty when the ecosystem's
    /// document does not tie the artifact to one here.
    pub version: String,
    /// The artifact's identity: an npm archive's integrity, another
    /// ecosystem's file name.
    pub artifact: String,
}

/// Who sent a publication request, and what identifies it for a retry.
#[derive(Debug, Clone)]
pub struct PublicationRequest {
    /// The stable principal that sent it.
    pub principal: String,
    /// The client's idempotency key, or one derived from `request_digest`
    /// for a client that sent none.
    pub idempotency_key: String,
    /// The digest of the whole request.
    pub request_digest: String,
    /// The name the publisher gave the publication.
    pub name: Option<String>,
}

/// What a committed request produced, as recorded for its retries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedOutcome {
    pub catalog: String,
    pub generation: u64,
    pub publication: Option<u64>,
}

/// The result of committing a publication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicationCommit {
    /// The generation that holds the request's documents.
    pub generation: Generation,
    /// The publication the commit recorded, when it introduced an artifact.
    pub publication: Option<u64>,
    /// The request had already committed, and this is its recorded outcome.
    pub replayed: bool,
}

/// How a publication came to be recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicationOrigin {
    /// A commit recorded it.
    Published,
    /// Synthesized for a version accepted outside any recorded publication.
    /// It is a best-known link, not evidence of how the version arrived.
    Imported,
}

/// A committed publication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Publication {
    pub catalog: String,
    /// The generation that committed it; for an imported one, the generation
    /// that accepted its version.
    pub id: u64,
    pub name: Option<String>,
    pub publisher: Option<String>,
    pub committed_at: String,
    pub origin: PublicationOrigin,
    pub members: Vec<ArtifactMember>,
}
