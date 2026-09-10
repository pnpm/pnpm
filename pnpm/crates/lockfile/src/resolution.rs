pub use git_hosted::is_git_hosted_tarball_url;
pub use registry::{
    RegistryContext, RegistryOptions, RegistryServerType, TarballUrlOptions,
    integrity_addressed_registry_tarball_url, is_integrity_addressed_registry_tarball_url,
    npm_tarball_url, pick_registry_for_package, registry_server_type, registry_supports_time_field,
};

use derive_more::{Display, Error, From, Into, TryInto};
use pipe_trait::Pipe;
use pnpm_crypto_hash::integrity_addressed_tarball_path;
use pnpm_diagnostics::miette::Diagnostic;
use serde::{Deserialize, Serialize};
use ssri::Integrity;
use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap},
};

/// For tarball hosted remotely or locally.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TarballResolution {
    pub tarball: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integrity: Option<Integrity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<TarballRevision>,
    /// `true` for tarballs sourced from a git host (codeload.github.com /
    /// gitlab.com / bitbucket.org). Such tarballs need preparation
    /// (preparePackage / packlist) on extraction, and their cached content
    /// depends on whether build scripts ran, so they are addressed by a
    /// git-hosted store-index key rather than the integrity-based key.
    ///
    /// The git resolver sets this when it produces the resolution; the
    /// lockfile loader back-fills it on entries whose URL matches a known
    /// git host for backward compatibility with lockfiles written before
    /// this field existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_hosted: Option<bool>,
    /// Sub-directory inside the tarball to pack. The git-hosted tarball
    /// fetcher uses it to package only one directory of a monorepo's
    /// archive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

impl TarballResolution {
    /// Whether the tarball is a git-provider archive, and so needs the
    /// prepare + packlist pass and the git-hosted store-index key.
    ///
    /// The recorded flag is a hint: pnpm's `classifyResolution` decides
    /// from the URL alone, so an archive URL from a known git host
    /// counts even when the lockfile omits the field (entries written
    /// before it existed) or contradicts it.
    #[must_use]
    pub fn is_git_hosted(&self) -> bool {
        self.git_hosted == Some(true) || is_git_hosted_tarball_url(&self.tarball)
    }
}

/// For standard package specification, with package name and version range.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RegistryResolution {
    pub integrity: Integrity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<TarballRevision>,
}

/// Largest registry revision every pnpm implementation can represent exactly.
pub const MAX_TARBALL_REVISION: u64 = 9_007_199_254_740_991;

/// A positive registry artifact revision in JavaScript's safe-integer range.
#[derive(
    Debug, Display, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Into,
)]
#[serde(try_from = "u64", into = "u64")]
pub struct TarballRevision(u64);

impl TarballRevision {
    #[must_use]
    pub fn get(self) -> u64 {
        self.0
    }
}

impl TryFrom<u64> for TarballRevision {
    type Error = InvalidTarballRevisionError;

    fn try_from(revision: u64) -> Result<Self, Self::Error> {
        if (1..=MAX_TARBALL_REVISION).contains(&revision) {
            Ok(Self(revision))
        } else {
            Err(InvalidTarballRevisionError { revision })
        }
    }
}

#[derive(Debug, Display, Error, Clone, Copy)]
#[display("tarball revision {revision} is not a positive integer at most {MAX_TARBALL_REVISION}")]
pub struct InvalidTarballRevisionError {
    pub revision: u64,
}

/// A resolver-produced tarball revision that cannot be represented as the
/// compact, integrity-addressed lockfile form required by the RFC.
#[derive(Debug, Display, Error, Diagnostic, Clone, Copy)]
#[non_exhaustive]
pub enum LockfileFormError {
    #[display("Cannot serialize a tarball revision without integrity.")]
    #[diagnostic(code(ERR_PNPM_INVALID_TARBALL_REVISION))]
    RevisionWithoutIntegrity,

    #[display(
        "Cannot serialize tarball revision {revision}: its URL does not match its integrity and registry."
    )]
    #[diagnostic(code(ERR_PNPM_INVALID_TARBALL_REVISION))]
    RevisionUrlMismatch { revision: TarballRevision },
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
    /// Accepted so that a lockfile carrying one still loads, and dropped
    /// on the next write.
    ///
    /// No pnpm version computes this: git content is pinned by `commit`,
    /// and the store key is git-hosted rather than integrity-addressed,
    /// so nothing checks a checkout against it. Re-emitting it would keep
    /// advertising a hash that was never verified — in the lockfile, and
    /// through `pnpm sbom` into a CycloneDX/SPDX `hashes` entry.
    ///
    /// Kept as an unvalidated `String` because the value is discarded:
    /// rejecting a malformed one would fail a lockfile pnpm reads fine.
    #[serde(default, skip_serializing)]
    pub integrity: Option<String>,
    /// Sub-directory inside the cloned tree to package. The git fetcher
    /// uses it so the build runs inside the sub-directory rather than the
    /// repo root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// One of the named executables a [`BinaryResolution`] exposes. The
/// lockfile records either a single string (one binary, named after the
/// package) or a map of `{ bin_name -> path_inside_archive }` so a
/// runtime archive can expose several launchers (e.g. `node` and
/// `node-mips`).
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
/// `tarball` is the common shape for nodejs.org's `.tar.gz` artifacts
/// (Linux / macOS); `zip` is what Windows Node ships as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BinaryArchive {
    Tarball,
    Zip,
}

/// For a downloaded binary archive (a JavaScript runtime: Node, Deno,
/// or Bun).
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
    /// `node-v22.0.0-darwin-arm64`). Only emitted for zip archives,
    /// where the resolver sets the prefix to the archive's basename.
    /// The zip extractor strips this prefix when applying
    /// `ignoreFilePattern` and renames the resulting
    /// `<temp>/<basename>/` directory to the CAS target. Tarball
    /// entries already carry the prefix in their tar header, so this
    /// stays `None` for them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
}

/// One `(os, cpu, libc?)` triple a [`PlatformAssetResolution`] covers.
///
/// `libc` is only written for musl-built variants; glibc is the
/// implicit default on Linux and the field is omitted everywhere
/// else. `Option<String>` (rather than `Option<Libc>` enum) keeps
/// future libc values future-compatible without a churning serde
/// migration if a new one lands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PlatformAssetTarget {
    pub os: String,
    pub cpu: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub libc: Option<String>,
}

/// One variant of a [`VariationsResolution`]: an inner [`LockfileResolution`]
/// paired with the host triples it covers.
///
/// The inner resolution is *atomic* in the on-disk shape — a
/// [`BinaryResolution`], [`TarballResolution`], etc. — never another
/// [`VariationsResolution`]. Pacquet's type is wider (the full
/// [`LockfileResolution`]) for serde-round-trip uniformity, and we trust
/// the lockfile to honor that contract: [`select_platform_variant`] does
/// not add a runtime check rejecting a nested `Variations`. A malformed
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
///
/// At install time, the dispatcher walks `variants` in declaration
/// order and picks the first whose `targets[]` includes the host
/// triple — see [`select_platform_variant`] in this module.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct VariationsResolution {
    pub variants: Vec<PlatformAssetResolution>,
}

/// The `type` tag of a [`CustomResolution`]. Any tag other than the
/// built-in kinds (`directory`, `git`, `binary`, `variations`) is
/// custom — matching `classifyResolution` in
/// `pnpm11/resolving/resolver-base/src/index.ts`, which routes every
/// unrecognized `type` to the custom fetch path. Rejecting the built-in
/// tags here keeps a malformed built-in resolution (e.g. a `git` entry
/// missing `commit`) a hard parse error instead of silently
/// reclassifying it as custom.
#[derive(Debug, Display, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CustomResolutionType(String);

impl CustomResolutionType {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for CustomResolutionType {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "directory" | "git" | "binary" | "variations" => {
                Err(format!("`{value}` is a built-in resolution type, not a custom one"))
            }
            _ => Ok(Self(value)),
        }
    }
}

impl From<CustomResolutionType> for String {
    fn from(value: CustomResolutionType) -> Self {
        value.0
    }
}

/// A resolution whose `type` is not one of the built-in kinds —
/// produced by a custom resolver from a pnpmfile and fetchable only by
/// a custom fetcher. The object is preserved verbatim (key order
/// included, via `serde_json`'s `preserve_order`) so the pnpmfile's
/// fetcher sees exactly what its resolver wrote to the lockfile.
///
/// Mirrors the TypeScript interface of the same name in
/// `pnpm11/resolving/resolver-base/src/index.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomResolution {
    #[serde(rename = "type")]
    pub resolution_type: CustomResolutionType,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Host triple used to pick a variant out of a [`VariationsResolution`].
///
/// `libc`'s tri-state encodes the `string | null | undefined` shape:
///
/// - `None` — the host's libc constraint is irrelevant (macOS, Windows,
///   BSD, ...). Matches a variant whose `libc` is `None` (the default
///   build); a `libc: "musl"` variant is rejected since `musl` is a
///   non-default, non-interchangeable artifact.
/// - `Some("glibc")` — Linux with glibc. Same matching rule as `None`:
///   the default variant wins, musl variants are skipped. `null` and
///   `"glibc"` collapse into the same arm because the variant emitter
///   only annotates non-glibc builds.
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
/// `None` if no variant matches.
///
/// Iterates `variants` in declaration order and returns the first
/// [`PlatformAssetResolution`] whose `targets[]` contains an `(os, cpu,
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
/// selector's `libc` value.
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
    Custom(CustomResolution),
}

impl LockfileResolution {
    /// Get the integrity field if available.
    #[must_use]
    pub fn integrity(&self) -> Option<&'_ Integrity> {
        match self {
            LockfileResolution::Tarball(resolution) => resolution.integrity.as_ref(),
            LockfileResolution::Registry(resolution) => Some(&resolution.integrity),
            LockfileResolution::Binary(resolution) => Some(&resolution.integrity),
            // Directory resolutions have no integrity, and a git
            // resolution's recorded one is never a checkable hash — it is
            // accepted and dropped (see [`GitResolution::integrity`]).
            // Variations is a meta-shape — the integrity lives on the
            // picked variant's inner resolution, so callers must
            // resolve through `pick_variant` first. Custom resolutions
            // are opaque — whatever integrity scheme they carry belongs
            // to the custom fetcher, not the built-in verification.
            LockfileResolution::Directory(_)
            | LockfileResolution::Git(_)
            | LockfileResolution::Variations(_)
            | LockfileResolution::Custom(_) => None,
        }
    }

    /// [`Self::integrity`] narrowed to an integrity that can actually
    /// check downloaded bytes. An `integrity: ''` entry parses into zero
    /// hashes, which pins nothing — pnpm treats that as no integrity at
    /// all, and [`Integrity::check`] would panic on it.
    #[must_use]
    pub fn checkable_integrity(&self) -> Option<&'_ Integrity> {
        self.integrity().filter(|integrity| !integrity.hashes.is_empty())
    }

    /// Convert an in-memory resolution into the form written to the lockfile.
    ///
    /// For a registry tarball whose URL is reconstructible from `name`,
    /// `version`, and the registry, the URL is dropped and only `{integrity}`
    /// is kept — pnpm derives the tarball URL on demand. The URL is preserved
    /// when [`LockfileFormOptions::include_tarball_url`] is set, when it is a
    /// `file:` tarball, when it is git-hosted, or when it does not match the
    /// derived URL (e.g. private registries with non-standard tarball paths).
    /// Non-tarball resolutions and integrity-less tarballs pass through
    /// unchanged.
    pub fn to_lockfile_form(
        &self,
        name: &str,
        version: &str,
        opts: LockfileFormOptions<'_>,
    ) -> Result<LockfileResolution, LockfileFormError> {
        let LockfileFormOptions { registry, server_type, include_tarball_url } = opts;
        let LockfileResolution::Tarball(tarball) = self else { return Ok(self.clone()) };
        let Some(integrity) = tarball.integrity.as_ref() else {
            return if tarball.revision.is_some() {
                Err(LockfileFormError::RevisionWithoutIntegrity)
            } else {
                Ok(self.clone())
            };
        };

        let git_hosted = tarball.is_git_hosted();
        let integrity_addressed =
            is_integrity_addressed_registry_tarball_url(&tarball.tarball, integrity, registry);
        if let Some(revision) = tarball.revision.filter(|_| !integrity_addressed) {
            return Err(LockfileFormError::RevisionUrlMismatch { revision });
        }
        // A standard registry tarball whose URL can be rebuilt from name+version+
        // registry is written as just `{integrity}` — pnpm derives the URL on
        // demand. Every other tarball must keep its URL or it can no longer be
        // re-fetched on a frozen-lockfile install: `file:` tarballs, git-provider
        // tarballs, and non-standard registry URLs (npm Enterprise, GitHub Packages
        // `/download/` URLs). `include_tarball_url` forces the URL to be kept.
        let rebuildable = !git_hosted
            && !tarball.tarball.starts_with("file:")
            && (integrity_addressed
                || (!include_tarball_url
                    && tarball.revision.is_none()
                    && is_canonical_registry_tarball_url(
                        &tarball.tarball,
                        name,
                        version,
                        TarballUrlOptions { registry, server_type },
                    )));
        if rebuildable {
            return Ok(LockfileResolution::Registry(RegistryResolution {
                integrity: integrity.clone(),
                revision: tarball.revision,
            }));
        }
        // The kept-URL form carries the `git_hosted` marker and the subdirectory
        // `path` (`repo#commit&path:/sub/dir`, only ever set on git-hosted tarballs)
        // so a git-hosted monorepo tarball still unpacks the right subfolder.
        // See <https://github.com/pnpm/pnpm/issues/12304>.
        Ok(LockfileResolution::Tarball(TarballResolution {
            tarball: tarball.tarball.clone(),
            integrity: Some(integrity.clone()),
            revision: tarball.revision,
            git_hosted: git_hosted.then_some(true),
            path: tarball.path.clone(),
        }))
    }
}

/// Inputs to [`LockfileResolution::to_lockfile_form`].
#[derive(Debug, Clone, Copy)]
pub struct LockfileFormOptions<'a> {
    pub registry: &'a str,
    pub server_type: Option<RegistryServerType>,
    /// Keep the tarball URL even when it is reconstructible.
    pub include_tarball_url: bool,
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
///
/// `Custom` must stay the last untagged variant: it accepts any object
/// with a non-built-in `type` tag, so every built-in shape has to get
/// its chance to match (or to *fail loudly* — [`CustomResolutionType`]
/// rejects built-in tags, keeping a malformed built-in resolution a
/// parse error rather than a silent reclassification) first.
#[derive(Serialize, Deserialize, From, TryInto)]
#[serde(untagged)]
enum ResolutionSerde {
    Tarball(TarballResolution),
    Registry(RegistryResolution),
    Tagged(TaggedResolution),
    Custom(CustomResolution),
}

impl From<ResolutionSerde> for LockfileResolution {
    fn from(value: ResolutionSerde) -> Self {
        match value {
            ResolutionSerde::Tarball(mut resolution) => {
                // Back-fill `gitHosted` for entries written by older pnpm
                // versions that lacked the field.
                if resolution.git_hosted.is_none() && is_git_hosted_tarball_url(&resolution.tarball)
                {
                    resolution.git_hosted = Some(true);
                }
                resolution.into()
            }
            ResolutionSerde::Registry(resolution) => resolution.into(),
            ResolutionSerde::Tagged(TaggedResolution::Directory(resolution)) => resolution.into(),
            ResolutionSerde::Tagged(TaggedResolution::Git(mut resolution)) => {
                // Drop a recorded integrity the moment the entry is read, so
                // it can never reach the lockfile writer, an SBOM, or a
                // resolution comparison. See [`GitResolution::integrity`].
                resolution.integrity = None;
                resolution.into()
            }
            ResolutionSerde::Tagged(TaggedResolution::Binary(resolution)) => resolution.into(),
            ResolutionSerde::Tagged(TaggedResolution::Variations(resolution)) => resolution.into(),
            ResolutionSerde::Custom(resolution) => resolution.into(),
        }
    }
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
            LockfileResolution::Custom(resolution) => resolution.into(),
        }
    }
}

#[cfg(test)]
mod tests;

mod registry;

use registry::is_canonical_registry_tarball_url;

mod git_hosted;
