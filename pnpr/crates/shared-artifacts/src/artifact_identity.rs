use super::{
    ArtifactCandidate, ArtifactPayload, ArtifactSubject, CompatibilityConstraints, ObjectPath,
    OwnerScope, RegistryError, Result, Sha256,
};
use sha2::Digest as _;

pub(super) fn scopes_prefix(owner: &str, entry: &str) -> String {
    format!("{owner}/entries/{entry}/scopes/")
}

pub(super) fn scope_marker_path(owner: &str, entry: &str, scope: &str) -> String {
    format!("{}{scope}", scopes_prefix(owner, entry))
}

/// The scope a marker under an entry names, or `None` for anything else stored
/// there. Variants sit beside the marker directory, not inside it.
pub(super) fn scope_name(path: &ObjectPath) -> Option<&str> {
    let (parent, name) = path.as_ref().rsplit_once('/')?;
    parent.ends_with("/scopes").then_some(name)
}

/// Which organizations' artifacts a caller may read and publish. The server
/// decides from the organizations it declares; the store asks before every
/// lookup, download, and publication.
pub trait OrgAccess: Sync {
    /// The caller's name, for the error a refused request reports.
    fn username(&self) -> &str;
    fn may_read(&self, org: &str) -> bool;
    fn may_publish(&self, org: &str) -> bool;
}

/// The storage key of what `org` owns.
pub(super) fn org_key(org: &str) -> String {
    digest_segment(OwnerScope::organization(org).namespace().as_bytes())
}

/// The storage key of `owner`, when `caller` may read what it owns.
pub(super) fn owner_key(caller: &(impl OrgAccess + ?Sized), owner: &OwnerScope) -> Result<String> {
    authorized_owner_key(caller, owner, false)
}

/// The storage key of `owner`, when `caller` may publish what it owns.
pub(super) fn publisher_owner_key(
    caller: &(impl OrgAccess + ?Sized),
    owner: &OwnerScope,
) -> Result<String> {
    authorized_owner_key(caller, owner, true)
}

fn authorized_owner_key(
    caller: &(impl OrgAccess + ?Sized),
    owner: &OwnerScope,
    publish: bool,
) -> Result<String> {
    if let OwnerScope::Organization { name } = owner
        && (if publish { caller.may_publish(name) } else { caller.may_read(name) })
    {
        return Ok(org_key(name));
    }
    Err(RegistryError::Forbidden {
        user: caller.username().to_string(),
        action: if publish {
            "publish shared artifacts owned by"
        } else {
            "access shared artifacts owned by"
        },
        resource: owner.namespace(),
    })
}

pub(super) fn object_name(path: &ObjectPath) -> &str {
    path.as_ref()
        .rsplit('/')
        .next()
        .unwrap_or_default()
}

pub(super) fn is_variant_file(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() == 69
        && bytes[64..] == *b".json"
        && bytes[..64]
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

/// The record that a blob was uploaded on its own, ahead of the publication
/// that references it. Empty: only its age matters.
pub(super) fn staged_record_path(owner: &str, id: &str) -> String {
    format!("{owner}/staged/{id}")
}

/// The blob a staged record stands for, when `relative` is one.
pub(super) fn staged_blob_path(relative: &str) -> Option<String> {
    let mut segments = relative.split('/');
    let (Some(owner), Some("staged"), Some(blob), None) =
        (segments.next(), segments.next(), segments.next(), segments.next())
    else {
        return None;
    };
    (is_digest_segment(owner) && !blob.is_empty()).then(|| format!("{owner}/blobs/{blob}"))
}

pub(super) fn is_blob_path(relative: &str) -> bool {
    let mut segments = relative.split('/');
    let (Some(owner), Some("blobs"), Some(blob), None) =
        (segments.next(), segments.next(), segments.next(), segments.next())
    else {
        return false;
    };
    is_digest_segment(owner) && !blob.is_empty()
}

pub(super) fn entry_owner(relative: &str) -> Option<&str> {
    let mut segments = relative.split('/');
    let (Some(owner), Some("entries"), Some(entry), Some(variant), None) =
        (segments.next(), segments.next(), segments.next(), segments.next(), segments.next())
    else {
        return None;
    };
    (is_digest_segment(owner) && is_digest_segment(entry) && is_variant_file(variant)).then_some(
        owner,
    )
}

pub(super) fn is_digest_segment(segment: &str) -> bool {
    segment.len() == 64
        && segment
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn artifact_operation_id() -> Result<String> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|error| RegistryError::Internal {
            reason: format!("could not generate a shared artifact operation ID: {error}"),
        })?;
    Ok(hex(&bytes))
}

pub(super) fn digest_segment(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

pub(super) fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            use std::fmt::Write as _;
            write!(output, "{byte:02x}").expect("writing to a String cannot fail");
            output
        })
}

/// The slot an artifact claims within its entry: one per set of compatibility
/// constraints, so a `universal` build and a glibc-2.31 build coexist while two
/// builds advertising the same constraints do not.
///
/// Hex-encoded so that it has the shape [`is_variant_file`] recognises, which
/// envelope digests share.
pub(super) fn compatibility_slot(compatibility: &CompatibilityConstraints) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"pnpm-shared-artifact-slot-v1\0");
    match compatibility {
        CompatibilityConstraints::Universal => hasher.update(b"universal\0"),
        CompatibilityConstraints::Tagged { tags } => {
            hasher.update(b"tagged\0");
            // Sorted because matching a tag set is order-independent: two
            // orderings are the same constraint, and hashing them apart would
            // hand the same platform two slots to be published into. The
            // protocol already rejects duplicates, so sorting canonicalizes.
            let mut tags: Vec<&str> = tags
                .iter()
                .map(String::as_str)
                .collect();
            tags.sort_unstable();
            for tag in tags {
                hasher.update(tag.as_bytes());
                hasher.update([0]);
            }
        }
    }
    hex(&hasher.finalize())
}

pub(super) fn entry_digest(key: &str, subject: &ArtifactSubject) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"pnpm-shared-artifact-entry-v1\0");
    hasher.update(key.as_bytes());
    hasher.update([0]);
    hasher.update(serde_json::to_vec(subject).expect("artifact subjects serialize"));
    hex(&hasher.finalize())
}

pub(super) fn artifact_matches_candidate(
    payload: &ArtifactPayload,
    candidate: &ArtifactCandidate,
) -> bool {
    let ArtifactCandidate { key: input_key, subject, owner } = candidate;
    payload.input_key == *input_key && payload.subject == *subject && payload.owner == *owner
}
