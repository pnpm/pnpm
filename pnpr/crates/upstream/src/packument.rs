use super::{
    CanonicalPackageName, DateTime, Integrity, MAX_TARBALL_REVISION, TarballRevision, Utc, Value,
    integrity_addressed_registry_tarball_url, is_integrity_addressed_registry_tarball_url,
};
use chrono::Timelike;

/// Rewrite every `dist.tarball` in `value` to a URL served by *this*
/// registry instead of whatever URL the source put there. The new URL is
/// `{public_url}/{pkg}/-/{basename}`, where `basename` is the last
/// `/`-separated segment of the original tarball URL. This handles both
/// npm's canonical `/{pkg}/-/{basename}` shape and verdaccio's
/// `/{scope}/{name}/-/{scope}/{filename}` shape uniformly — we only look
/// at the basename, never at the path prefix.
///
/// The basename is preserved verbatim rather than reconstructed from the
/// version, so a non-canonical tarball name (e.g. esprima-fb's zero-padded
/// `esprima-fb-3001.0001.0000-dev-harmony-fb.tgz` for version
/// `3001.1.0-dev-harmony-fb`) survives into the client's lockfile and is
/// fetched back from the path the upstream actually hosts. The tarball
/// endpoint binds each request to a version's declared `dist.integrity`
/// (see `serve_tarball`), so a preserved name can't smuggle in unverified
/// bytes.
///
/// Walks both packument shape (`{ "versions": { v: { dist: ... } } }`)
/// and single-version manifest shape (`{ dist: ... }` at the top level)
/// so a single helper covers both endpoints.
pub fn rewrite_tarball_urls(value: &mut Value, pkg: &CanonicalPackageName, public_url: &str) {
    rewrite_tarball_urls_from_registry(value, pkg, None, public_url);
}

/// Rewrite tarball URLs from an upstream registry, preserving valid revision routes.
pub fn rewrite_upstream_tarball_urls(
    value: &mut Value,
    pkg: &CanonicalPackageName,
    source_registry: &str,
    public_url: &str,
) {
    rewrite_tarball_urls_from_registry(value, pkg, Some(source_registry), public_url);
}

pub(super) fn rewrite_tarball_urls_from_registry(
    value: &mut Value,
    pkg: &CanonicalPackageName,
    source_registry: Option<&str>,
    public_url: &str,
) {
    let public_url = public_url.trim_end_matches('/');
    if let Some(versions) = value.get_mut("versions").and_then(Value::as_object_mut) {
        for manifest in versions.values_mut() {
            rewrite_dist_tarball(manifest, pkg, source_registry, public_url);
        }
    }
    rewrite_dist_tarball(value, pkg, source_registry, public_url);
}

pub(super) fn rewrite_dist_tarball(
    value: &mut Value,
    pkg: &CanonicalPackageName,
    source_registry: Option<&str>,
    public_url: &str,
) {
    // Every string `dist.tarball` must be rewritten to a route on *this*
    // server, where integrity and OSV are enforced — never passed through.
    // When the upstream URL has no usable basename (e.g. it ends in `/`), fall
    // back to the version-derived canonical name (the manifest carries its own
    // `version`) so a malformed URL still points at pnpr (and 404s there)
    // rather than directing the client at an arbitrary upstream host.
    let fallback = value
        .get("version")
        .and_then(Value::as_str)
        .map(|version| pkg.tarball_name_for_version(version));
    let Some(dist) = value.get_mut("dist").and_then(Value::as_object_mut) else {
        return;
    };
    if let Some(source_registry) = source_registry {
        rewrite_upstream_revision_tarball_urls(dist, source_registry, public_url);
    }
    let revision_url = source_registry
        .and_then(|source_registry| unique_revision_url(dist, source_registry, public_url));
    if let Some(revision_url) = revision_url {
        let Some(tarball_value) = dist.get_mut("tarball") else { return };
        *tarball_value = Value::String(revision_url);
        return;
    }
    dist.remove("revision");
    let Some(tarball_value) = dist.get_mut("tarball") else { return };
    if !tarball_value.is_string() {
        return;
    }
    let filename = tarball_value
        .as_str()
        .and_then(tarball_basename)
        .map(str::to_owned)
        .or(fallback)
        .unwrap_or_default();
    *tarball_value = Value::String(format!("{public_url}/{}/-/{filename}", pkg.as_str()));
}

/// The integrity-addressed tarball route on this server for the packument's
/// pinned revision, when exactly one listed revision matches it.
pub(super) fn unique_revision_url(
    dist: &serde_json::Map<String, Value>,
    source_registry: &str,
    public_url: &str,
) -> Option<String> {
    let revision = dist.get("revision")?.as_u64()?;
    TarballRevision::try_from(revision).ok()?;
    let integrity: Integrity = dist.get("integrity")?.as_str()?.parse().ok()?;
    let tarball = dist.get("tarball")?.as_str()?;
    if !is_integrity_addressed_registry_tarball_url(tarball, &integrity, source_registry) {
        return None;
    }
    let revision_url = integrity_addressed_registry_tarball_url(&integrity, public_url)?;
    let matches = dist
        .get("revisions")?
        .as_array()?
        .iter()
        .filter(|entry| {
            entry.get("revision").and_then(Value::as_u64) == Some(revision)
                && entry
                    .get("integrity")
                    .and_then(Value::as_str)
                    .and_then(|integrity| integrity.parse::<Integrity>().ok())
                    .is_some_and(|candidate| candidate == integrity)
                && entry.get("tarball").and_then(Value::as_str) == Some(revision_url.as_str())
        })
        .count();
    (matches == 1).then_some(revision_url)
}

pub(super) fn rewrite_upstream_revision_tarball_urls(
    dist: &mut serde_json::Map<String, Value>,
    source_registry: &str,
    public_url: &str,
) {
    let Some(revisions) = dist.get_mut("revisions").and_then(Value::as_array_mut) else {
        return;
    };
    revisions.retain_mut(|revision| {
        revision
            .as_object_mut()
            .is_some_and(|revision| rewrite_revision_tarball(revision, source_registry, public_url))
    });
}

/// Point one revision's tarball at pnpr, dropping the revision when anything
/// about it fails to check out against the source registry.
pub(super) fn rewrite_revision_tarball(
    revision: &mut serde_json::Map<String, Value>,
    source_registry: &str,
    public_url: &str,
) -> bool {
    let Some(number) = revision.get("revision").and_then(Value::as_u64) else {
        return false;
    };
    if number > MAX_TARBALL_REVISION || !revision.get("manifest").is_some_and(Value::is_object) {
        return false;
    }
    let Some(integrity) = revision
        .get("integrity")
        .and_then(Value::as_str)
        .and_then(|integrity| integrity.parse::<Integrity>().ok())
    else {
        return false;
    };
    let addressed = revision.get("tarball").and_then(Value::as_str).is_some_and(|tarball| {
        is_integrity_addressed_registry_tarball_url(tarball, &integrity, source_registry)
    });
    if !addressed {
        return false;
    }
    let Some(tarball) = integrity_addressed_registry_tarball_url(&integrity, public_url) else {
        return false;
    };
    revision.insert("tarball".to_string(), Value::String(tarball));
    true
}

/// The tarball filename a `dist.tarball` URL points at: the final path
/// segment, with any `?query`/`#fragment` stripped. This basename is the
/// trust key shared by the rewritten public URL ([`rewrite_tarball_urls`])
/// and the serve-time version match (`expected_tarball_dist`), so both
/// must derive it identically — including for query-bearing URLs (signed
/// CDN links), where the query is not part of the route path a client
/// later requests. Returns `None` for a URL whose path ends in `/`.
#[must_use]
pub fn tarball_basename(url: &str) -> Option<&str> {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    path.rsplit('/').next().filter(|segment| !segment.is_empty())
}

/// Look up the version manifest for `version_or_tag` inside a parsed
/// packument: if the string matches a dist-tag it resolves through
/// `dist-tags[tag]` first, otherwise it's taken as a literal version.
/// Returns the version's manifest *with* the `dist.tarball` rewritten
/// to point at this server.
#[must_use]
pub fn extract_version_manifest(
    packument: &Value,
    pkg: &CanonicalPackageName,
    version_or_tag: &str,
    public_url: &str,
) -> Option<Value> {
    extract_version_manifest_from_registry(packument, pkg, version_or_tag, None, public_url)
}

/// Extract a version manifest while preserving valid upstream revision routes.
#[must_use]
pub fn extract_upstream_version_manifest(
    packument: &Value,
    pkg: &CanonicalPackageName,
    version_or_tag: &str,
    source_registry: &str,
    public_url: &str,
) -> Option<Value> {
    extract_version_manifest_from_registry(
        packument,
        pkg,
        version_or_tag,
        Some(source_registry),
        public_url,
    )
}

pub(super) fn extract_version_manifest_from_registry(
    packument: &Value,
    pkg: &CanonicalPackageName,
    version_or_tag: &str,
    source_registry: Option<&str>,
    public_url: &str,
) -> Option<Value> {
    let resolved = packument
        .get("dist-tags")
        .and_then(|tags| tags.get(version_or_tag))
        .and_then(Value::as_str)
        .unwrap_or(version_or_tag);
    let mut manifest = packument.get("versions")?.get(resolved)?.clone();
    rewrite_tarball_urls_from_registry(&mut manifest, pkg, source_registry, public_url);
    Some(manifest)
}

/// Top-level packument fields *copied verbatim* into the abbreviated
/// (`application/vnd.npm.install-v1+json`) form.
///
/// `time` isn't here because it's coarsened rather than copied — see
/// [`coarsen_time_map`]. It goes beyond the npm spec but the
/// pnpm/pacquet resolvers read it for the `minimumReleaseAge` check,
/// so it stays (in a shrunken form).
///
/// `modified` isn't here either because it's synthesized: it's
/// extracted from `time.modified` (real npm packuments nest it
/// there). pacquet reads `meta.modified` in its version-pick
/// heuristics (`pick_package_from_meta.rs`) and as a freshness check
/// (`pick_package.rs`); omitting it pushes the resolver onto a slower
/// fallback path.
pub(super) const ABBREVIATED_TOP_FIELDS: &[&str] = &["name", "dist-tags"];

/// Age past which a `time` entry loses its time-of-day in the
/// abbreviated form, keeping only the (rounded-up) bare date.
/// `minimumReleaseAge` cutoffs sit in the recent past (days, not
/// weeks), so a week-old entry rounded up to the next day is still
/// unambiguously on the mature side of any realistic cutoff. See
/// [`coarsen_time_map`].
pub(super) const TIME_PRECISION_HORIZON_DAYS: i64 = 7;

/// Per-version fields preserved in the abbreviated form — a subset of
/// the npm spec's abbreviated version object
/// (<https://github.com/npm/registry/blob/ae49abf1ba/docs/responses/package-metadata.md#abbreviated-version-object>).
/// Fields neither the pnpm nor the pacquet resolver reads are dropped
/// to shrink the document: `funding`, `acceptDependencies`,
/// `_hasShrinkwrap`, and `devDependencies` (a dependency's dev
/// dependencies are never installed). Redundant `dist` subfields are
/// trimmed per-version by [`trim_dist_fields`].
pub(super) const ABBREVIATED_VERSION_FIELDS: &[&str] = &[
    "name",
    "version",
    "deprecated",
    "bin",
    "dist",
    "engines",
    "directories",
    "dependencies",
    "peerDependencies",
    "optionalDependencies",
    "bundleDependencies",
    "cpu",
    "os",
    "libc",
    "peerDependenciesMeta",
    "hasInstallScript",
];

/// Strip a parsed packument down to the abbreviated install-v1 form.
/// Should be called *after* [`rewrite_tarball_urls`] so the returned
/// document's `dist.tarball` URLs already point at this server.
pub fn abbreviate_packument(packument: &Value, now: DateTime<Utc>) -> Value {
    let mut out = serde_json::Map::new();
    let Some(obj) = packument.as_object() else {
        return Value::Object(out);
    };
    copy_fields(&mut out, obj, ABBREVIATED_TOP_FIELDS);
    // Coarsen `time`, then synthesize `modified` from the coarsened map —
    // npm packuments nest `modified` under `time` and pacquet's resolver
    // reads it at the top level.
    if let Some(time) = obj.get("time").and_then(Value::as_object) {
        let time = coarsen_time_map(time, now);
        if let Some(modified) = time.get("modified") {
            out.insert("modified".to_string(), modified.clone());
        }
        out.insert("time".to_string(), Value::Object(time));
    }
    if let Some(versions) = obj.get("versions").and_then(Value::as_object) {
        out.insert("versions".to_string(), abbreviate_versions(versions));
    }
    Value::Object(out)
}

/// The `versions` map with each manifest cut down to the fields a resolver
/// reads.
pub(super) fn abbreviate_versions(versions: &serde_json::Map<String, Value>) -> Value {
    let mut abbreviated = serde_json::Map::with_capacity(versions.len());
    for (version_id, version_value) in versions {
        let Some(version_obj) = version_value.as_object() else { continue };
        let mut trimmed = serde_json::Map::new();
        copy_fields(&mut trimmed, version_obj, ABBREVIATED_VERSION_FIELDS);
        trim_dist_fields(&mut trimmed);
        abbreviated.insert(version_id.clone(), Value::Object(trimmed));
    }
    Value::Object(abbreviated)
}

/// Copy whichever of `fields` the source carries.
pub(super) fn copy_fields(
    out: &mut serde_json::Map<String, Value>,
    source: &serde_json::Map<String, Value>,
    fields: &[&str],
) {
    for &field in fields {
        if let Some(value) = source.get(field) {
            out.insert(field.to_string(), value.clone());
        }
    }
}

/// Trim `dist` subfields the resolver and installer never read:
///
/// * `npm-signature` — the legacy PGP detached signature. npm stopped
///   populating it years ago in favour of the ECDSA `signatures`, and
///   nothing in pnpm or pacquet reads it.
/// * `shasum` — the legacy sha1 hash, redundant once `integrity` (SRI)
///   is present. "Present" mirrors pnpm's `getIntegrity` truthiness
///   check (`if (dist.integrity)`): a non-empty string. An absent,
///   empty, or non-string `integrity` keeps `shasum` so pnpm's
///   sha1 fallback still has a hash (pre-2017 publishes).
///
/// `dist.signatures` (the ECDSA registry signatures) is deliberately
/// preserved: it binds `name@version:integrity` to the upstream
/// registry's key and is the input to a potential client-side
/// install-time verification on the pnpr path.
///
/// `unpackedSize` and `fileCount` are preserved: pacquet reads both
/// off the resolver-fetched manifest — `unpackedSize` sizes the
/// decompression buffer, and together they form the download's
/// queueing priority (the estimated pipeline work that starts the
/// most expensive tarballs first).
pub(super) fn trim_dist_fields(version: &mut serde_json::Map<String, Value>) {
    let Some(dist) = version.get_mut("dist").and_then(Value::as_object_mut) else {
        return;
    };
    dist.remove("npm-signature");
    if dist.get("integrity").and_then(Value::as_str).is_some_and(|integrity| !integrity.is_empty())
    {
        dist.remove("shasum");
    }
}

/// Shrink a packument `time` map by dropping precision the resolvers
/// don't need: seconds come off every timestamp, and entries older
/// than [`TIME_PRECISION_HORIZON_DAYS`] lose the time-of-day entirely
/// (down to the bare `YYYY-MM-DD`). Responses go out uncompressed, so
/// every character dropped is a byte off the wire.
///
/// Both reduced forms stay parseable by pnpm (`new Date`) and pacquet
/// (`pnpm_resolving_resolver_base::parse_packument_timestamp`).
/// Values are rounded *up* (see [`coarsen_timestamp`]) so the
/// maturity- and trust-checks that read them stay fail-safe.
/// Non-timestamp entries (the reserved `unpublished` object) and any
/// value pnpr can't parse as RFC 3339 pass through untouched.
pub(super) fn coarsen_time_map(
    time: &serde_json::Map<String, Value>,
    now: DateTime<Utc>,
) -> serde_json::Map<String, Value> {
    let horizon = now - chrono::Duration::days(TIME_PRECISION_HORIZON_DAYS);
    let mut out = serde_json::Map::with_capacity(time.len());
    for (key, value) in time {
        let coarsened = value.as_str().and_then(|raw| coarsen_timestamp(raw, horizon));
        out.insert(key.clone(), coarsened.map_or_else(|| value.clone(), Value::String));
    }
    out
}

/// Re-render one RFC 3339 timestamp at reduced precision, **rounding
/// up**: a bare date (the next day, unless already midnight) when it
/// predates `horizon`, otherwise the next whole minute (unless already
/// on a minute boundary). Rounding up keeps the coarsened value at or
/// after the real publish time, so `minimumReleaseAge` and trust
/// checks can only ever read a version as *newer* than it is — the
/// fail-safe direction (a too-new version is never coarsened into
/// looking mature). Returns `None` for strings that aren't RFC 3339 so
/// the caller keeps the original verbatim.
pub(super) fn coarsen_timestamp(raw: &str, horizon: DateTime<Utc>) -> Option<String> {
    let parsed = DateTime::parse_from_rfc3339(raw).ok()?.with_timezone(&Utc);
    if parsed < horizon {
        let date = parsed.date_naive();
        let rounded =
            if parsed == date.and_hms_opt(0, 0, 0)?.and_utc() { date } else { date.succ_opt()? };
        Some(rounded.format("%Y-%m-%d").to_string())
    } else {
        let minute = parsed.with_second(0)?.with_nanosecond(0)?;
        let rounded = if parsed == minute { minute } else { minute + chrono::Duration::minutes(1) };
        Some(rounded.format("%Y-%m-%dT%H:%MZ").to_string())
    }
}
