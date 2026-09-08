use crate::Digest;
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, collections::HashSet};

/// One manifest the repository holds, keyed by the digest of its bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestEntry {
    pub digest: Digest,
    pub media_type: String,
    pub size: u64,
    /// Missing metadata is populated from the manifest during referrer discovery.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub referrer: Option<ReferrerMetadata>,
}

/// Metadata used to discover artifacts attached to a subject manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferrerMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<Digest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_type_digest: Option<Digest>,
}

/// One tag and the manifest it currently names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagEntry {
    pub tag: String,
    pub digest: Digest,
    /// When this tag last moved, in milliseconds since the Unix epoch.
    ///
    /// A tag is the one mutable thing in a repository, so "what is already
    /// here wins" — the rule that makes an immutable version safe to
    /// re-apply — would let a transaction recovered after a crash drag a tag
    /// back to an older manifest. Comparing when instead makes the merge
    /// monotonic, so re-applying an old write is a no-op.
    ///
    /// A number rather than a formatted date, because the comparison is the
    /// whole point of the field: two spellings of one instant would otherwise
    /// order as different ones.
    ///
    /// A tie goes to the incoming write. Live writes to one repository are
    /// serialized by its package lock, so two of them landing in the same
    /// millisecond are still ordered, and the later one has to win or a push
    /// that answered `201` would leave the tag where it was. Clocks on two
    /// instances can still disagree, which is part of the cross-replica write
    /// story tracked in
    /// [pnpm/pnpm#12199](https://github.com/pnpm/pnpm/issues/12199).
    pub updated: u64,
}

/// The stored shape, which [`ImageDocument`] sorts on the way in.
///
/// It is a separate type so that the ordering that document guarantees is
/// established for every caller who deserializes one, not only for the one
/// that calls [`ImageDocument::parse`].
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredImageDocument {
    name: String,
    #[serde(default)]
    manifests: Vec<ManifestEntry>,
    #[serde(default)]
    tags: Vec<TagEntry>,
    #[serde(default)]
    generation: u64,
    #[serde(default)]
    deleting_blob: Option<Digest>,
}

impl From<StoredImageDocument> for ImageDocument {
    fn from(stored: StoredImageDocument) -> Self {
        let StoredImageDocument { name, mut manifests, mut tags, generation, deleting_blob } =
            stored;
        manifests.sort_by(|left, right| left.digest.hex().cmp(right.digest.hex()));
        tags.sort_by(|left, right| left.tag.cmp(&right.tag));
        Self { name, manifests, tags, generation, deleting_blob }
    }
}

/// What a hosted registry stores per image repository.
///
/// Layer and config blobs are absent on purpose: they are content-addressed,
/// immutable, and uploaded before anything points at them. The manifest is
/// what publishes a release, so it is what the document records.
///
/// Both collections are kept sorted, by digest and by tag, because every
/// lookup here is a binary search: a document whose entries reached storage
/// in another order would hide entries it holds. The ordering holds however
/// a document was built, not only on the paths that maintain it.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", from = "StoredImageDocument")]
pub struct ImageDocument {
    pub name: String,
    #[serde(default)]
    manifests: Vec<ManifestEntry>,
    #[serde(default)]
    tags: Vec<TagEntry>,
    /// Fences journaled publishes prepared before an explicit blob deletion.
    #[serde(default)]
    pub generation: u64,
    /// Blocks new publishes until the deletion completes or offline collection recovers it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleting_blob: Option<Digest>,
}

impl ImageDocument {
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self { name: name.to_string(), ..Self::default() }
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }

    /// The manifests this repository holds, ordered by digest.
    #[must_use]
    pub fn manifests(&self) -> &[ManifestEntry] {
        &self.manifests
    }

    /// The tags this repository holds, ordered by name.
    #[must_use]
    pub fn tags(&self) -> &[TagEntry] {
        &self.tags
    }

    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("image document serializes")
    }

    #[must_use]
    pub fn manifest(&self, digest: &Digest) -> Option<&ManifestEntry> {
        self.manifests
            .binary_search_by(|entry| entry.digest.hex().cmp(digest.hex()))
            .ok()
            .map(|index| &self.manifests[index])
    }

    #[must_use]
    pub fn tag(&self, tag: &str) -> Option<&TagEntry> {
        self.tags
            .binary_search_by(|entry| entry.tag.as_str().cmp(tag))
            .ok()
            .map(|index| &self.tags[index])
    }

    /// The manifest a `<reference>` path segment names, whether it spells a
    /// digest or a tag.
    #[must_use]
    pub fn resolve(&self, reference: &str) -> Option<&ManifestEntry> {
        match Digest::parse(reference) {
            Ok(digest) => self.manifest(&digest),
            Err(_) => self.tag(reference).and_then(|tag| self.manifest(&tag.digest)),
        }
    }

    /// Tag names in lexical order, as `GET /v2/<name>/tags/list` serves them.
    #[must_use]
    pub fn tag_names(&self) -> Vec<&str> {
        self.tags.iter().map(|entry| entry.tag.as_str()).collect()
    }

    pub fn insert_manifest(&mut self, entry: ManifestEntry) {
        match self.manifests.binary_search_by(|held| held.digest.hex().cmp(entry.digest.hex())) {
            Ok(index) => self.manifests[index] = entry,
            Err(index) => self.manifests.insert(index, entry),
        }
    }

    pub fn set_tag(&mut self, entry: TagEntry) {
        match self.tags.binary_search_by(|held| held.tag.cmp(&entry.tag)) {
            Ok(index) => self.tags[index] = entry,
            Err(index) => self.tags.insert(index, entry),
        }
    }

    /// Every tag that named the manifest goes with it. `false` when the
    /// repository held no such manifest.
    pub fn remove_manifest(&mut self, digest: &Digest) -> bool {
        let Ok(index) = self.manifests.binary_search_by(|held| held.digest.hex().cmp(digest.hex()))
        else {
            return false;
        };
        self.manifests.remove(index);
        self.tags.retain(|tag| &tag.digest != digest);
        true
    }

    /// The manifest the tag named stays, still reachable by digest. `false`
    /// when the repository held no such tag.
    pub fn remove_tag(&mut self, tag: &str) -> bool {
        let Ok(index) = self.tags.binary_search_by(|held| held.tag.as_str().cmp(tag)) else {
            return false;
        };
        self.tags.remove(index);
        true
    }

    /// Take on everything in `addition` this document does not already hold,
    /// skipping whatever points at a blob in `lost_blobs`, and report
    /// whether that changed anything.
    ///
    /// Manifests are content-addressed, so one already here is the same
    /// bytes and is left alone. Tags reach the same result whichever order
    /// the two documents arrive in, which is what lets a journaled write be
    /// replayed after the one that superseded it.
    ///
    /// A generation mismatch or pending blob deletion refuses the addition
    /// and returns `false`, just like an unchanged document. Older generations
    /// cannot be replayed after deletion advances the stored generation.
    pub fn merge(&mut self, addition: Self, lost_blobs: &HashSet<String>) -> bool {
        if self.generation != addition.generation || self.deleting_blob.is_some() {
            return false;
        }
        let mut changed = false;
        if self.name.is_empty() && !addition.name.is_empty() {
            self.name.clone_from(&addition.name);
            changed = true;
        }
        changed |= self.merge_manifests(addition.manifests, lost_blobs);
        changed |= self.merge_tags(addition.tags, lost_blobs);
        changed
    }

    /// Take every manifest this document does not already hold and whose blob
    /// the transaction did not lose.
    fn merge_manifests(
        &mut self,
        manifests: Vec<ManifestEntry>,
        lost_blobs: &HashSet<String>,
    ) -> bool {
        let mut changed = false;
        for entry in manifests {
            if lost_blobs.contains(&entry.digest.blob_filename())
                || self.manifest(&entry.digest).is_some()
            {
                continue;
            }
            self.insert_manifest(entry);
            changed = true;
        }
        changed
    }

    /// Take every tag mapping that supersedes the one held, skipping any whose
    /// manifest this document does not have.
    fn merge_tags(&mut self, tags: Vec<TagEntry>, lost_blobs: &HashSet<String>) -> bool {
        let mut changed = false;
        for entry in tags {
            if lost_blobs.contains(&entry.digest.blob_filename())
                || self.manifest(&entry.digest).is_none()
                || !self.tag_supersedes(&entry)
            {
                continue;
            }
            self.set_tag(entry);
            changed = true;
        }
        changed
    }

    /// A newer write always wins, even where it names the digest the tag
    /// already holds: dropping it would leave the older timestamp in place,
    /// and a stale journaled write replayed later would then compare as newer
    /// and move the tag backward. On a tie only a different digest is a
    /// change, so re-applying the mapping already held costs no document
    /// write.
    fn tag_supersedes(&self, entry: &TagEntry) -> bool {
        self.tag(&entry.tag).is_none_or(|held| match held.updated.cmp(&entry.updated) {
            Ordering::Less => true,
            Ordering::Equal => held.digest != entry.digest,
            Ordering::Greater => false,
        })
    }
}
