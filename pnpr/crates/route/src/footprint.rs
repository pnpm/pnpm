use super::{BTreeSet, Digest, HeaderMap, Identity, RouteContext, Sha256};

/// The cache-namespace identity of a private route: a key input plus
/// (for the cache layer) an authorization gate. The key input is what
/// gets HMAC'd into the cache key; identical inputs from different
/// callers who share the same access collapse to one shared entry.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum PrivateAccessDescriptor {
    /// Proxied route via a pnpr-managed upstream alias. [`credential_digest`]
    /// hashes the upstream's `Authorization`, so rotating the credential moves
    /// future hits to a new namespace — no manual epoch counter to bump.
    /// `package` is set only when the upstream's rules **explicitly refine**
    /// this name's `access`: the descriptor then re-checks that per-package
    /// gate on cache replay, so a caller the refinement denies cannot obtain
    /// through the cache what a fresh resolve refuses them. Unrefined names
    /// share the plain registry-scoped descriptor (`package: None`).
    Alias { alias: String, credential_digest: String, package: Option<String> },
    /// pnpr-hosted route, gated by re-running the named package access
    /// policy for the caller.
    Hosted { policy_id: String },
}

impl PrivateAccessDescriptor {
    /// The stable, collision-resistant bytes that go into the cache-key
    /// HMAC. `\0` separates fields so distinct shapes can't alias (an
    /// alias literally named `hosted` can't collide with a hosted
    /// policy of the same text).
    pub(super) fn key_input(&self) -> String {
        match self {
            PrivateAccessDescriptor::Alias { alias, credential_digest, package: None } => {
                format!("alias\0{alias}\0{credential_digest}")
            }
            PrivateAccessDescriptor::Alias { alias, credential_digest, package: Some(package) } => {
                format!("alias\0{alias}\0{credential_digest}\0{package}")
            }
            PrivateAccessDescriptor::Hosted { policy_id } => format!("hosted\0{policy_id}"),
        }
    }

    /// The metadata-namespace id for this single descriptor: an HMAC over
    /// its [`Self::key_input`] keyed with the server `secret`, so the
    /// on-disk private-metadata mirror path is not correlatable offline.
    /// Distinct from [`Footprint::digest`], which combines *all* of a
    /// resolution's descriptors — metadata is fetched one route at a time,
    /// so each fetch keys on its own descriptor.
    pub(super) fn digest_id(&self, secret: &[u8]) -> String {
        hex(&hmac_sha256(secret, self.key_input().as_bytes()))
    }
}

/// The HMAC digest namespacing an upstream's private cache (packuments and
/// tarballs), identical to the metadata-mirror descriptor id for the same
/// `(upstream, credential)`. Keyed by the server `secret` so the on-disk path
/// reveals neither the upstream name nor its credential, and so a path-unsafe
/// upstream name (`..`, `/`) can never escape the cache root — the digest is
/// hex. The digest passed here is a [`headers_credential_digest`] over the
/// upstream's attached headers, so rotating any credential moves to a fresh
/// namespace.
#[must_use]
pub fn upstream_cache_digest(upstream: &str, credential_digest: String, secret: &[u8]) -> String {
    PrivateAccessDescriptor::Alias { alias: upstream.to_string(), credential_digest, package: None }
        .digest_id(secret)
}

/// A hash of an upstream's `Authorization` header value, used as the credential
/// epoch in [`PrivateAccessDescriptor::Alias`]. Rotating the upstream
/// credential changes this automatically, re-keying every private cache the
/// upstream owns — so a rotation invalidates old-credential content with no
/// manual step to forget. The raw token never appears: it's a one-way SHA-256
/// (then HMAC-keyed with the server secret by `PrivateAccessDescriptor::digest_id`
/// before it reaches disk).
#[must_use]
pub fn credential_digest(authorization: &str) -> String {
    hex(&Sha256::digest(authorization.as_bytes()))
}

/// A hash of an upstream's *effective credential material* — every request header
/// it attaches upstream, not just `Authorization` — used as the credential epoch
/// in [`PrivateAccessDescriptor::Alias`]. pnpr supports credentials carried in
/// custom headers, so rotating any of them must re-key the upstream's private
/// cache; keying on `Authorization` alone would keep serving old-credential
/// content after such a rotation.
///
/// Stable across runs and independent of map iteration order: the (name, value)
/// pairs are sorted and length-prefixed before hashing, so the encoding of a
/// header set is unambiguous and distinct sets can only collide by a SHA-256
/// collision. The raw values never reach disk — this is a one-way SHA-256,
/// then HMAC-keyed with the server secret by
/// `PrivateAccessDescriptor::digest_id`.
#[must_use]
pub fn headers_credential_digest(headers: &HeaderMap) -> String {
    let mut entries: Vec<(&[u8], &[u8])> =
        headers.iter().map(|(name, value)| (name.as_str().as_bytes(), value.as_bytes())).collect();
    entries.sort_unstable();
    let mut hasher = Sha256::new();
    for (name, value) in entries {
        hasher.update((name.len() as u64).to_le_bytes());
        hasher.update(name);
        hasher.update((value.len() as u64).to_le_bytes());
        hasher.update(value);
    }
    hex(&hasher.finalize())
}

/// The set of private routes a single resolve actually touched, paired
/// with the descriptor selected for each. Accumulated during resolution
/// through the [`crate::RouteHook`]; consumed afterwards to decide how the
/// resolution may be cached.
///
/// An empty footprint means the resolution is fully public and shareable
/// globally; a non-empty one is keyed by its private access descriptors.
#[derive(Debug, Default, Clone)]
pub struct Footprint {
    pub(super) descriptors: BTreeSet<PrivateAccessDescriptor>,
}

impl Footprint {
    pub fn add(&mut self, descriptor: PrivateAccessDescriptor) {
        self.descriptors.insert(descriptor);
    }

    /// Whether the resolution touched no private data and may be cached
    /// under the global, auth-excluded key.
    #[must_use]
    pub fn is_public(&self) -> bool {
        self.descriptors.is_empty()
    }

    /// The private-key component for this footprint: an HMAC over the
    /// sorted union of its descriptors, keyed with the server `secret`
    /// so the key is not correlatable offline. `None` for a public
    /// footprint (no private descriptors), in which case the cache uses
    /// the global auth-excluded key.
    #[must_use]
    pub fn digest(&self, secret: &[u8]) -> Option<String> {
        if self.descriptors.is_empty() {
            return None;
        }
        let mut message = String::new();
        for descriptor in &self.descriptors {
            message.push_str(&descriptor.key_input());
            message.push('\n');
        }
        Some(hex(&hmac_sha256(secret, message.as_bytes())))
    }

    /// Whether every private descriptor in this footprint is still
    /// authorized for `identity` under the current route context.
    #[must_use]
    pub fn allows(&self, context: &RouteContext, identity: &Identity) -> bool {
        self.descriptors.iter().all(|descriptor| context.allows_descriptor(identity, descriptor))
    }
}

/// HMAC-SHA256 (RFC 2104) over the workspace's audited `sha2`, so the
/// descriptor digest needs no extra crypto dependency. Verified against
/// the RFC 4231 test vectors in the unit tests.
pub(super) fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut block_key = [0u8; BLOCK];
    if key.len() > BLOCK {
        let digest = Sha256::digest(key);
        block_key[..digest.len()].copy_from_slice(&digest);
    } else {
        block_key[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    let mut outer = Sha256::new();
    let mut inner_pad = [0u8; BLOCK];
    let mut outer_pad = [0u8; BLOCK];
    for index in 0..BLOCK {
        inner_pad[index] = block_key[index] ^ 0x36;
        outer_pad[index] = block_key[index] ^ 0x5c;
    }
    inner.update(inner_pad);
    inner.update(message);
    outer.update(outer_pad);
    outer.update(inner.finalize());
    outer.finalize().into()
}

pub(super) fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}
