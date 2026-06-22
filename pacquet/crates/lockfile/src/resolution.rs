use derive_more::{From, TryInto};
use pipe_trait::Pipe;
use serde::{Deserialize, Serialize};
use ssri::Integrity;
use std::collections::{BTreeMap, HashMap};

/// For tarball hosted remotely or locally.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TarballResolution {
    pub tarball: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integrity: Option<Integrity>,
    /// `true` for tarballs sourced from a git host (codeload.github.com /
    /// gitlab.com / bitbucket.org). Such tarballs need preparation
    /// (preparePackage / packlist) on extraction, and their cached content
    /// depends on whether build scripts ran, so they are addressed by a
    /// git-hosted store-index key rather than the integrity-based key.
    ///
    /// The git resolver sets this when it produces the resolution; the
    /// lockfile loader back-fills it on entries whose URL matches a known
    /// git host for backward compatibility with lockfiles written before
    /// this field existed. Mirrors pnpm's `TarballResolution.gitHosted`
    /// at <https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/types/src/index.ts#L88-L107>.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_hosted: Option<bool>,
    /// Sub-directory inside the tarball to pack, mirroring
    /// `GitResolution.path`. Pnpm's git-hosted tarball fetcher uses it
    /// to package only one directory of a monorepo's archive. Mirrors
    /// pnpm's `TarballResolution.path` at
    /// <https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/types/src/index.ts#L93>.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// For standard package specification, with package name and version range.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RegistryResolution {
    pub integrity: Integrity,
}

/// For local directory on a filesystem.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DirectoryResolution {
    pub directory: String,
}

/// For git repository.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GitResolution {
    pub repo: String,
    pub commit: String,
    /// Sub-directory inside the cloned tree to package. Mirrors pnpm's
    /// `GitRepositoryResolution.path` at
    /// <https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/types/src/index.ts#L120-L125>.
    /// The git fetcher passes this to `preparePackage` so the build runs
    /// inside the sub-directory rather than the repo root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// One of the named executables a [`BinaryResolution`] exposes. Pnpm
/// writes either a single string (one binary, named after the
/// package) or a map of `{ bin_name -> path_inside_archive }` so a
/// runtime archive can expose several launchers (e.g. `node` and
/// `node-mips`). Mirrors pnpm's
/// [`BinaryResolution.bin`](https://github.com/pnpm/pnpm/blob/94240bc046/resolving/resolver-base/src/index.ts#L46-L48)
/// type union.
///
/// `BTreeMap` (not `HashMap`) keeps the serialised order stable so a
/// round-trip through pacquet doesn't churn the lockfile diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BinarySpec {
    /// Single executable. The bin name defaults to the package name
    /// at install time; this string is the path *inside the archive*
    /// to the executable.
    Single(String),
    /// Named map of `bin_name -> path_inside_archive`.
    Map(BTreeMap<String, String>),
}

/// Archive format for a [`BinaryResolution`].
///
/// Mirrors pnpm's `BinaryResolution.archive` discriminator at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/resolving/resolver-base/src/index.ts#L47>.
/// `tarball` is the common shape for nodejs.org's `.tar.gz` artifacts
/// (Linux / macOS); `zip` is what Windows Node ships as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BinaryArchive {
    Tarball,
    Zip,
}

/// For a downloaded binary archive (a JavaScript runtime: Node, Deno,
/// or Bun). Mirrors pnpm's
/// [`BinaryResolution`](https://github.com/pnpm/pnpm/blob/94240bc046/resolving/resolver-base/src/index.ts#L41-L49).
///
/// The install path extracts the archive into the CAS (with optional
/// per-package `ignoreFilePattern` filtering — Node strips bundled
/// `npm` / `corepack`) and links the executables named in `bin` into
/// the importer's `node_modules/.bin/`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BinaryResolution {
    pub url: String,
    pub integrity: Integrity,
    pub bin: BinarySpec,
    pub archive: BinaryArchive,
    /// Basename of the archive's top-level directory (e.g.
    /// `node-v22.0.0-darwin-arm64`). Only emitted for zip archives —
    /// see
    /// [`engine/runtime/node-resolver/src/index.ts`](https://github.com/pnpm/pnpm/blob/94240bc046/engine/runtime/node-resolver/src/index.ts)
    /// where the resolver sets `resolution.prefix = address.basename`
    /// only for the `.zip` branch. The zip extractor strips this
    /// prefix when applying `ignoreFilePattern` and renames the
    /// resulting `<temp>/<basename>/` directory to the CAS target.
    /// Tarball entries already carry the prefix in their tar header,
    /// so this stays `None` for them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
}

/// One `(os, cpu, libc?)` triple a [`PlatformAssetResolution`] covers.
/// Mirrors pnpm's
/// [`PlatformAssetTarget`](https://github.com/pnpm/pnpm/blob/94240bc046/resolving/resolver-base/src/index.ts#L60-L64).
///
/// Pnpm only writes `libc` for musl-built variants; glibc is the
/// implicit default on Linux and the field is omitted everywhere
/// else. `Option<String>` (rather than `Option<Libc>` enum) keeps
/// future libc values future-compatible without a churning serde
/// migration if upstream adds one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PlatformAssetTarget {
    pub os: String,
    pub cpu: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub libc: Option<String>,
}

/// One variant of a [`VariationsResolution`]: an inner [`LockfileResolution`]
/// paired with the host triples it covers. Mirrors pnpm's
/// [`PlatformAssetResolution`](https://github.com/pnpm/pnpm/blob/94240bc046/resolving/resolver-base/src/index.ts#L66-L69).
///
/// The inner resolution is *atomic* upstream — a `BinaryResolution`,
/// `TarballResolution`, etc. — never another `VariationsResolution`.
/// Pacquet's type is wider (the full [`LockfileResolution`]) for serde-
/// round-trip uniformity, and we trust the lockfile to honor the
/// upstream contract: [`select_platform_variant`] does not add a
/// runtime check rejecting a nested `Variations`. A malformed
/// lockfile that nested them would just route the picked variant's
/// inner shape back through the install dispatcher, which surfaces
/// each shape independently — no infinite recursion is possible
/// because the install dispatcher does not call back into
/// [`select_platform_variant`] for non-`Variations` inputs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PlatformAssetResolution {
    pub resolution: LockfileResolution,
    pub targets: Vec<PlatformAssetTarget>,
}

/// For a runtime (or any platform-conditioned binary) that has more
/// than one downloadable artifact, one per `(os, cpu, libc?)` combo.
/// Mirrors pnpm's
/// [`VariationsResolution`](https://github.com/pnpm/pnpm/blob/94240bc046/resolving/resolver-base/src/index.ts#L73-L76).
///
/// At install time, the dispatcher walks `variants` in declaration
/// order and picks the first whose `targets[]` includes the host
/// triple — see [`select_platform_variant`] in this module.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct VariationsResolution {
    pub variants: Vec<PlatformAssetResolution>,
}

/// Host triple used to pick a variant out of a [`VariationsResolution`].
/// Mirrors pnpm's
/// [`PlatformSelector`](https://github.com/pnpm/pnpm/blob/94240bc046/resolving/resolver-base/src/index.ts#L78-L83).
///
/// `libc`'s tri-state encodes pnpm's `string | null | undefined` shape:
///
/// - `None` — the host's libc constraint is irrelevant (macOS, Windows,
///   BSD, ...). Matches a variant whose `libc` is `None` (the default
///   build); a `libc: "musl"` variant is rejected since `musl` is a
///   non-default, non-interchangeable artifact.
/// - `Some("glibc")` — Linux with glibc. Same matching rule as `None`:
///   the default variant wins, musl variants are skipped. Upstream
///   collapses `null` and `"glibc"` into the same arm in
///   [`libcMatches`](https://github.com/pnpm/pnpm/blob/94240bc046/resolving/resolver-base/src/index.ts#L100-L107)
///   because the variant emitter only annotates non-glibc builds.
/// - `Some("musl")` — Linux with musl. Requires an exact `libc:
///   "musl"` annotation on the variant, so the glibc default doesn't
///   silently install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformSelector {
    pub os: String,
    pub cpu: String,
    pub libc: Option<String>,
}

/// Pick the variant whose target list contains the host triple, or
/// `None` if no variant matches. Port of pnpm's
/// [`selectPlatformVariant`](https://github.com/pnpm/pnpm/blob/94240bc046/resolving/resolver-base/src/index.ts#L92-L98).
///
/// Iterates `variants` in declaration order and returns the first
/// `PlatformAssetResolution` whose `targets[]` contains an `(os, cpu,
/// libc?)` triple matching `selector`. Each variant's target list is
/// scanned linearly — `targets[]` is typically 1–3 entries (one per
/// architecture combo that shares an artifact), so the nested-loop
/// cost is negligible.
#[must_use]
pub fn select_platform_variant<'a>(
    variants: &'a [PlatformAssetResolution],
    selector: &PlatformSelector,
) -> Option<&'a PlatformAssetResolution> {
    variants.iter().find(|variant| {
        variant.targets.iter().any(|target| {
            target.os == selector.os
                && target.cpu == selector.cpu
                && libc_matches(target.libc.as_deref(), selector.libc.as_deref())
        })
    })
}

/// Check whether a variant's `libc` annotation matches the host
/// selector's `libc` value. Port of upstream's
/// [`libcMatches`](https://github.com/pnpm/pnpm/blob/94240bc046/resolving/resolver-base/src/index.ts#L100-L107).
///
/// The contract is asymmetric on purpose: `None` and `"glibc"` on the
/// selector side both demand `None` on the variant (the unannotated
/// default), so a `musl` variant cannot win for a glibc host. A
/// non-default selector value (e.g. `"musl"`) requires the variant to
/// declare the exact same value.
pub(crate) fn libc_matches(variant_libc: Option<&str>, requested_libc: Option<&str>) -> bool {
    match requested_libc {
        None | Some("glibc") => variant_libc.is_none(),
        Some(requested) => variant_libc == Some(requested),
    }
}

/// Represent the resolution object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, From, TryInto)]
#[serde(from = "ResolutionSerde", into = "ResolutionSerde")]
pub enum LockfileResolution {
    Tarball(TarballResolution),
    Registry(RegistryResolution),
    Directory(DirectoryResolution),
    Git(GitResolution),
    Binary(BinaryResolution),
    Variations(VariationsResolution),
}

impl LockfileResolution {
    /// Get the integrity field if available.
    #[must_use]
    pub fn integrity(&self) -> Option<&'_ Integrity> {
        match self {
            LockfileResolution::Tarball(resolution) => resolution.integrity.as_ref(),
            LockfileResolution::Registry(resolution) => Some(&resolution.integrity),
            LockfileResolution::Binary(resolution) => Some(&resolution.integrity),
            // Directory / Git resolutions have no integrity.
            // Variations is a meta-shape — the integrity lives on the
            // picked variant's inner resolution, so callers must
            // resolve through `pick_variant` first.
            LockfileResolution::Directory(_)
            | LockfileResolution::Git(_)
            | LockfileResolution::Variations(_) => None,
        }
    }

    /// Convert an in-memory resolution into the form written to the lockfile.
    ///
    /// For a registry tarball whose URL is reconstructible from `name`,
    /// `version`, and `registry`, the URL is dropped and only `{integrity}` is
    /// kept — pnpm derives the tarball URL on demand. The URL is preserved when
    /// `include_tarball_url` is set, when it is a `file:` tarball, when it is
    /// git-hosted, or when it does not match the derived URL (e.g. private
    /// registries with non-standard tarball paths). Non-tarball resolutions and
    /// integrity-less tarballs pass through unchanged.
    ///
    /// Port of pnpm's
    /// [`toLockfileResolution`](https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/utils/src/toLockfileResolution.ts).
    #[must_use]
    pub fn to_lockfile_form(
        &self,
        name: &str,
        version: &str,
        registry: &str,
        include_tarball_url: bool,
    ) -> LockfileResolution {
        let LockfileResolution::Tarball(tarball) = self else { return self.clone() };
        let Some(integrity) = tarball.integrity.as_ref() else { return self.clone() };

        let git_hosted =
            tarball.git_hosted == Some(true) || is_git_hosted_tarball_url(&tarball.tarball);
        // A standard registry tarball whose URL can be rebuilt from name+version+
        // registry is written as just `{integrity}` — pnpm derives the URL on
        // demand. Every other tarball must keep its URL or it can no longer be
        // re-fetched on a frozen-lockfile install: `file:` tarballs, git-provider
        // tarballs, and non-standard registry URLs (npm Enterprise, GitHub Packages
        // `/download/` URLs). `include_tarball_url` forces the URL to be kept.
        if !include_tarball_url
            && !git_hosted
            && !tarball.tarball.starts_with("file:")
            && is_canonical_registry_tarball_url(&tarball.tarball, name, version, registry)
        {
            return LockfileResolution::Registry(RegistryResolution {
                integrity: integrity.clone(),
            });
        }
        // The kept-URL form carries the `git_hosted` marker and the subdirectory
        // `path` (`repo#commit&path:/sub/dir`, only ever set on git-hosted tarballs)
        // so a git-hosted monorepo tarball still unpacks the right subfolder.
        // See <https://github.com/pnpm/pnpm/issues/12304>.
        LockfileResolution::Tarball(TarballResolution {
            tarball: tarball.tarball.clone(),
            integrity: Some(integrity.clone()),
            git_hosted: git_hosted.then_some(true),
            path: tarball.path.clone(),
        })
    }
}

/// Derive the canonical npm registry tarball URL for `name@version`. Port of
/// the [`get-npm-tarball-url`](https://www.npmjs.com/package/get-npm-tarball-url)
/// package pnpm uses.
#[must_use]
pub fn npm_tarball_url(name: &str, version: &str, registry: &str) -> String {
    let registry =
        if registry.ends_with('/') { registry.to_string() } else { format!("{registry}/") };
    let scopeless = match name.strip_prefix('@') {
        Some(scoped) => scoped.split_once('/').map_or(name, |(_, bare)| bare),
        None => name,
    };
    let version = version.split_once('+').map_or(version, |(base, _)| base);
    format!("{registry}{name}/-/{scopeless}-{version}.tgz")
}

/// Whether `tarball` is the canonical npm registry URL derived from `name`,
/// `version`, and `registry` — i.e. it can be dropped from the lockfile and
/// rebuilt on demand. Percent-encoding is case-insensitive, so the unescape
/// matches both `%2f` and `%2F` in the URLs npm produces for scoped packages.
fn is_canonical_registry_tarball_url(
    tarball: &str,
    name: &str,
    version: &str,
    registry: &str,
) -> bool {
    let expected = npm_tarball_url(name, version, registry);
    let actual = tarball.replace("%2f", "/").replace("%2F", "/");
    remove_protocol(&expected) == remove_protocol(&actual)
}

/// Default-vs-scope routing for an npm package. Mirrors pnpm's
/// [`pickRegistryForPackage`](https://github.com/pnpm/pnpm/blob/main/config/pick-registry-for-package/src/index.ts).
///
/// Routing rules:
///
/// 1. **`npm:` alias.** When `bare_specifier` is an `npm:` alias the
///    *alias target* decides routing, not the local key:
///    - `npm:@scope/name@<spec>` → `registries[@scope]`.
///    - `npm:name@<spec>` (unscoped target) → `registries["default"]`,
///      never the local alias's scope, because the fetched package is
///      unscoped and doesn't live on a scoped registry.
/// 2. **Plain spec.** Falls back to `pkg_name`'s scope when present;
///    otherwise `registries["default"]`.
#[must_use]
pub fn pick_registry_for_package(
    registries: &HashMap<String, String>,
    pkg_name: &str,
    bare_specifier: Option<&str>,
) -> String {
    let scope = match bare_specifier.and_then(|spec| spec.strip_prefix("npm:")) {
        Some(target) => scope_of(target),
        None => scope_of(pkg_name),
    };
    if let Some(scope) = scope
        && let Some(url) = registries.get(scope)
    {
        return url.clone();
    }
    registries.get("default").cloned().unwrap_or_default()
}

fn scope_of(name: &str) -> Option<&str> {
    if !name.starts_with('@') {
        return None;
    }
    name.find('/').map(|sep| &name[..sep])
}

/// Strip only a leading `http://` or `https://` scheme (case-insensitive) so
/// URLs are compared protocol-insensitively, without truncating on a later
/// `://` in the path or query. Port of pnpm's `removeProtocol`
/// (`url.replace(/^https?:\/\//i, '')`).
fn remove_protocol(url: &str) -> &str {
    ["https://", "http://"]
        .into_iter()
        .find_map(|scheme| {
            url.get(..scheme.len())
                .filter(|head| head.eq_ignore_ascii_case(scheme))
                .map(|_| &url[scheme.len()..])
        })
        .unwrap_or(url)
}

/// Intermediate helper type for serde.
#[derive(Serialize, Deserialize, From, TryInto)]
#[serde(tag = "type", rename_all = "camelCase")]
enum TaggedResolution {
    Directory(DirectoryResolution),
    Git(GitResolution),
    Binary(BinaryResolution),
    Variations(VariationsResolution),
}

/// Intermediate helper type for serde.
#[derive(Serialize, Deserialize, From, TryInto)]
#[serde(untagged)]
enum ResolutionSerde {
    Tarball(TarballResolution),
    Registry(RegistryResolution),
    Tagged(TaggedResolution),
}

impl From<ResolutionSerde> for LockfileResolution {
    fn from(value: ResolutionSerde) -> Self {
        match value {
            ResolutionSerde::Tarball(mut resolution) => {
                // Back-fill `gitHosted` for entries written by older pnpm
                // versions that lacked the field. Mirrors upstream's
                // `enrichGitHostedFlag` at
                // <https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/fs/src/lockfileFormatConverters.ts#L158-L168>.
                if resolution.git_hosted.is_none() && is_git_hosted_tarball_url(&resolution.tarball)
                {
                    resolution.git_hosted = Some(true);
                }
                resolution.into()
            }
            ResolutionSerde::Registry(resolution) => resolution.into(),
            ResolutionSerde::Tagged(TaggedResolution::Directory(resolution)) => resolution.into(),
            ResolutionSerde::Tagged(TaggedResolution::Git(resolution)) => resolution.into(),
            ResolutionSerde::Tagged(TaggedResolution::Binary(resolution)) => resolution.into(),
            ResolutionSerde::Tagged(TaggedResolution::Variations(resolution)) => resolution.into(),
        }
    }
}

/// Recognizes immutable archive URLs emitted by known git providers. The result
/// gates integrity exemptions, so path shapes are matched explicitly and refs
/// must be full commit SHAs.
#[must_use]
pub fn is_git_hosted_tarball_url(url: &str) -> bool {
    let Some((host, path, query)) = parse_https_url(url) else { return false };
    if host.eq_ignore_ascii_case("codeload.github.com") {
        return is_github_codeload_archive(path);
    }
    if host.eq_ignore_ascii_case("bitbucket.org") {
        return is_bitbucket_archive(path);
    }
    if host.eq_ignore_ascii_case("gitlab.com") {
        return is_gitlab_archive(path, query);
    }
    false
}

fn parse_https_url(url: &str) -> Option<(&str, &str, Option<&str>)> {
    const HTTPS_SCHEME: &str = "https://";
    if !url.get(..HTTPS_SCHEME.len())?.eq_ignore_ascii_case(HTTPS_SCHEME) {
        return None;
    }
    let rest = url.get(HTTPS_SCHEME.len()..)?;
    let (host, path_and_query) = rest.split_once('/')?;
    let path_and_query = path_and_query.split_once('#').map_or(path_and_query, |(path, _)| path);
    let (path, query) = path_and_query
        .split_once('?')
        .map_or((path_and_query, None), |(path, query)| (path, Some(query)));
    Some((host, path, query))
}

fn is_github_codeload_archive(path: &str) -> bool {
    let segments = path_segments(path);
    segments.len() == 4 && segments[2] == "tar.gz" && is_full_commit_sha(segments[3])
}

fn is_bitbucket_archive(path: &str) -> bool {
    let segments = path_segments(path);
    if segments.len() != 4 || segments[2] != "get" {
        return false;
    }
    let Some(commit) = segments[3].strip_suffix(".tar.gz") else { return false };
    is_full_commit_sha(commit)
}

fn is_gitlab_archive(path: &str, query: Option<&str>) -> bool {
    let segments = path_segments(path);
    if segments.len() == 6
        && segments[0] == "api"
        && segments[1] == "v4"
        && segments[2] == "projects"
        && segments[4] == "repository"
        && segments[5] == "archive.tar.gz"
    {
        return query_param(query, "ref").is_some_and(is_full_commit_sha);
    }
    let Some(archive_marker_index) =
        segments.windows(2).position(|window| window[0] == "-" && window[1] == "archive")
    else {
        return false;
    };
    if archive_marker_index < 2 || segments.len() != archive_marker_index + 4 {
        return false;
    }
    let commit = segments[archive_marker_index + 2];
    let archive_name = segments[archive_marker_index + 3];
    archive_name.ends_with(".tar.gz") && is_full_commit_sha(commit)
}

fn path_segments(path: &str) -> Vec<&str> {
    path.split('/').filter(|segment| !segment.is_empty()).collect()
}

fn query_param<'query>(query: Option<&'query str>, key: &str) -> Option<&'query str> {
    query?.split('&').find_map(|part| {
        let (part_key, value) = part.split_once('=')?;
        (part_key == key).then_some(value)
    })
}

fn is_full_commit_sha(value: &str) -> bool {
    value.len() == 40 && value.as_bytes().iter().all(u8::is_ascii_hexdigit)
}

impl From<LockfileResolution> for ResolutionSerde {
    fn from(value: LockfileResolution) -> Self {
        match value {
            LockfileResolution::Tarball(resolution) => resolution.into(),
            LockfileResolution::Registry(resolution) => resolution.into(),
            LockfileResolution::Directory(resolution) => {
                resolution.pipe(TaggedResolution::from).into()
            }
            LockfileResolution::Git(resolution) => resolution.pipe(TaggedResolution::from).into(),
            LockfileResolution::Binary(resolution) => {
                resolution.pipe(TaggedResolution::from).into()
            }
            LockfileResolution::Variations(resolution) => {
                resolution.pipe(TaggedResolution::from).into()
            }
        }
    }
}

#[cfg(test)]
mod tests;
