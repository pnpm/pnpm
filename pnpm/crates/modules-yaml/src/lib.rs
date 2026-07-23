//! Read and write pnpm's `node_modules/.modules.yaml` manifest.
//!
//! The manifest is stored at `<modules_dir>/.modules.yaml`, where
//! `modules_dir` is the path of a `node_modules` directory. The on-disk
//! format is JSON (which YAML accepts), so reads use a YAML parser and
//! writes emit [`serde_json::to_string_pretty`] output to match pnpm exactly.

use derive_more::{Display, Error, From, Into};
use indexmap::IndexSet;
use pacquet_diagnostics::miette::{self, Diagnostic};
use pacquet_fs::lexical_normalize;
use pipe_trait::Pipe;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs, io, iter,
    path::{Path, PathBuf},
    time::SystemTime,
};

/// Filename of the modules manifest inside `node_modules/`.
///
/// The leading dot is required because `npm shrinkwrap` would otherwise
/// treat the file as an extraneous package.
pub const MODULES_FILENAME: &str = ".modules.yaml";

/// Default value for the `virtualStoreDirMaxLength` field.
pub const DEFAULT_VIRTUAL_STORE_DIR_MAX_LENGTH: u64 = 120;

/// Capability trait: read a file's contents into a [`String`].
///
/// One trait per filesystem capability so each function declares only what
/// it actually uses, and so test fakes only implement the methods that
/// will be exercised. Pattern follows the per-capability typeclass style
/// rather than `parallel-disk-usage`'s lumped `FsApi` at
/// <https://github.com/KSXGitHub/parallel-disk-usage/blob/2aa39917f9/src/app/hdd.rs#L29-L35>.
pub trait FsReadToString {
    fn read_to_string(path: &Path) -> io::Result<String>;
}

/// Capability trait: create a directory and any missing parents.
pub trait FsCreateDirAll {
    fn create_dir_all(path: &Path) -> io::Result<()>;
}

/// Capability trait: write bytes to a file, replacing existing contents.
pub trait FsWrite {
    fn write(path: &Path, contents: &[u8]) -> io::Result<()>;
}

/// Capability trait: read the current wall-clock time as a [`SystemTime`].
///
/// Decoupled from [`SystemTime::now`] so tests can fake the clock and
/// assert deterministic `prunedAt` values.
pub trait Clock {
    fn now() -> SystemTime;
}

/// Production implementation, backed by [`std::fs`] and [`SystemTime::now`].
pub struct Host;

impl FsReadToString for Host {
    #[inline]
    fn read_to_string(path: &Path) -> io::Result<String> {
        fs::read_to_string(path)
    }
}

impl FsCreateDirAll for Host {
    #[inline]
    fn create_dir_all(path: &Path) -> io::Result<()> {
        fs::create_dir_all(path)
    }
}

impl FsWrite for Host {
    #[inline]
    fn write(path: &Path, contents: &[u8]) -> io::Result<()> {
        fs::write(path, contents)
    }
}

impl Clock for Host {
    #[inline]
    fn now() -> SystemTime {
        SystemTime::now()
    }
}

/// Newtype wrapper around a dependency-path string.
///
/// [`DepPath`] is a branded string: every construction site is unvalidated, so
/// there are no validating constructors. The brand exists purely to stop a
/// plain `string` from being assigned where a [`DepPath`] is expected at
/// compile time. No validation runs at construction, and
/// `#[serde(transparent)]` makes the wire format identical to `String` so a
/// [`DepPath`] round-trips through JSON / YAML the same way a plain string
/// does.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, From, Into,
)]
#[serde(transparent)]
pub struct DepPath(String);

impl DepPath {
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Typed view of a `node_modules/.modules.yaml` manifest.
///
/// This is the normalized shape: `ignoredBuilds` is widened from the on-disk
/// `DepPath[]` array to an in-memory set. Pacquet keeps the raw and
/// normalized shapes in a single struct: serde handles the
/// array↔[`IndexSet`] conversion at the [`Self::ignored_builds`]
/// field via [`IndexSet`]'s deduplicating `Deserialize` impl, so a
/// separate raw-shape type is not needed. `IndexSet` (insertion-ordered)
/// is chosen over `HashSet` / `BTreeSet` to match JavaScript `Set`'s
/// iteration semantics — the on-disk array order round-trips
/// byte-for-byte.
///
/// Every required field carries a `#[serde(default)]` so
/// legacy manifests written by older pnpm versions still deserialize;
/// the read path then fills in the modern shape from the legacy fields.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Modules {
    /// Legacy: the v5-era flat alias map, kept for read-side
    /// compatibility. Replaced by [`Self::hoisted_dependencies`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hoisted_aliases: Option<BTreeMap<DepPath, Vec<String>>>,

    /// `HoistedDependencies` is keyed by `DepPath | ProjectId`. Pacquet keeps
    /// the key as [`String`] because [`DepPath`] and `ProjectId` share the
    /// same underlying type with no validation, so the union cannot be
    /// disambiguated statically; the [`String`] type faithfully represents
    /// that union.
    #[serde(default)]
    pub hoisted_dependencies: BTreeMap<String, BTreeMap<String, HoistKind>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hoist_pattern: Option<Vec<String>>,

    #[serde(default)]
    pub included: IncludedDependencies,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout_version: Option<LayoutVersion>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_linker: Option<NodeLinker>,

    #[serde(default)]
    pub package_manager: String,

    #[serde(default)]
    pub pending_builds: Vec<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ignored_builds: Option<IndexSet<DepPath>>,

    #[serde(default)]
    pub pruned_at: String,

    // TODO: the strict manifest shape that the write path takes tightens
    // this to a required `Registries`. Revisit when the install-pipeline
    // port supplies a producer that always populates `default`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registries: Option<BTreeMap<String, String>>,

    /// Legacy: the v5-era flag used to mean "hoist everything publicly."
    /// Replaced by [`Self::public_hoist_pattern`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shamefully_hoist: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_hoist_pattern: Option<Vec<String>>,

    #[serde(default)]
    pub skipped: Vec<String>,

    #[serde(default)]
    pub store_dir: String,

    #[serde(default)]
    pub virtual_store_dir: String,

    #[serde(default)]
    pub virtual_store_dir_max_length: u64,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub injected_deps: Option<BTreeMap<String, Vec<String>>>,

    /// Per-depPath list of lockfile-relative directory paths where
    /// the package was placed under `nodeLinker: hoisted`. Required
    /// by rebuild (which throws `MISSING_HOISTED_LOCATIONS` when
    /// absent) and consulted by the hoisted dep-graph's skip-fetch
    /// optimization to decide whether the package is already on disk.
    /// An optional `Record<string, string[]>` on the on-disk shape.
    /// Pacquet's install pipeline does not populate this yet; the
    /// field is wired into the schema so a future hoisted-linker
    /// implementation can write it without changing the on-disk
    /// shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hoisted_locations: Option<BTreeMap<String, Vec<String>>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_builds: Option<BTreeMap<String, AllowBuildValue>>,

    /// `true` when this modules directory was populated by a
    /// `virtualStoreOnly` install (`pnpm fetch`). Such an install
    /// records empty hoist patterns because it did no hoisting, so the
    /// next ordinary install must finish the linking rather than read
    /// the pattern mismatch as drift and purge. Omitted when false,
    /// matching pnpm's delete-when-falsy encoding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub virtual_store_only: Option<bool>,
}

/// A lightweight version of [`Modules`] that skips deserializing the potentially
/// large `hoisted_dependencies` and `hoisted_locations` maps. Used during
/// the install fast path to check layout consistency without allocating massive
/// amounts of memory.
#[derive(Debug, Default, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModulesLayout {
    #[serde(default)]
    pub hoist_pattern: Option<Vec<String>>,
    #[serde(default)]
    pub included: IncludedDependencies,
    #[serde(default)]
    pub layout_version: Option<LayoutVersion>,
    #[serde(default)]
    pub node_linker: Option<NodeLinker>,
    #[serde(default)]
    pub package_manager: String,
    #[serde(default)]
    pub pending_builds: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ignored_builds: Option<IndexSet<DepPath>>,
    #[serde(default)]
    pub pruned_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registries: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_hoist_pattern: Option<Vec<String>>,
    /// Legacy: the v5-era flag used to mean "hoist everything publicly."
    /// Needed by [`read_modules_layout`] to apply the same normalization as
    /// [`read_modules_manifest`] and avoid false-positive layout mismatches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shamefully_hoist: Option<bool>,
    #[serde(default)]
    pub skipped: Vec<String>,
    #[serde(default)]
    pub store_dir: String,
    #[serde(default)]
    pub virtual_store_dir: String,
    #[serde(default)]
    pub virtual_store_dir_max_length: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_builds: Option<BTreeMap<String, AllowBuildValue>>,
    /// See [`Modules::virtual_store_only`]. Read by the layout-drift
    /// check, which skips the hoist-pattern comparison when it is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub virtual_store_only: Option<bool>,
}

/// Which dependency groups the install pipeline included.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncludedDependencies {
    #[serde(default)]
    pub dependencies: bool,
    #[serde(default)]
    pub dev_dependencies: bool,
    #[serde(default)]
    pub optional_dependencies: bool,
}

/// Linker variant the install pipeline used. The string variants match
/// pnpm's runtime values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeLinker {
    Hoisted,
    Isolated,
    Pnp,
}

/// Pinned identifier for the `node_modules` layout pacquet emits.
///
/// The unit type carries no data: its existence is the value. It serializes
/// as the integer `5` and deserializes only when the on-disk value is
/// exactly `5`. Any other version causes a deserialization error, the
/// breaking-change reaction to a missing or mismatched `layoutVersion`.
/// Wrapping this in [`Option`] on [`Modules`] distinguishes "missing"
/// (legacy, breaking change) from "present and matching".
///
/// The `#[serde(try_from = "u32", into = "u32")]` proxy lets us reuse
/// serde's number deserializer, while the [`TryFrom`] impl owns the
/// "is this version supported" decision and returns
/// [`UnsupportedLayoutVersionError`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct LayoutVersion;

impl LayoutVersion {
    /// The single layout version pacquet supports.
    const VALUE: u32 = 5;
}

impl From<LayoutVersion> for u32 {
    fn from(_: LayoutVersion) -> u32 {
        LayoutVersion::VALUE
    }
}

impl TryFrom<u32> for LayoutVersion {
    type Error = UnsupportedLayoutVersionError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        if value == LayoutVersion::VALUE {
            Ok(Self)
        } else {
            Err(UnsupportedLayoutVersionError { found: value })
        }
    }
}

/// Returned by [`LayoutVersion::try_from`] when the on-disk `layoutVersion`
/// is not the one pacquet supports.
#[derive(Debug, Display, Error)]
#[display(
    "Unsupported layout version {found}; this build of pnpm only supports layout version {}",
    LayoutVersion::VALUE
)]
pub struct UnsupportedLayoutVersionError {
    pub found: u32,
}

/// Per-alias visibility selected by the legacy `shamefullyHoist` flag.
/// Serializes as `"public"` or `"private"` to match the JSON shape pnpm
/// stores in `hoistedDependencies`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HoistKind {
    Public,
    Private,
}

/// Value stored under an [`Modules::allow_builds`] entry. pnpm
/// allows either a boolean toggle or a string allowlist label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AllowBuildValue {
    Bool(bool),
    String(String),
}

/// Error returned by [`read_modules_manifest`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum ReadModulesError {
    #[display("Failed to read {path:?}: {source}")]
    #[diagnostic(code(ERR_PNPM_MODULES_YAML_READ_IO))]
    ReadFile { path: PathBuf, source: io::Error },

    #[display("Failed to parse {path:?}: {source}")]
    #[diagnostic(code(ERR_PNPM_MODULES_YAML_PARSE_YAML))]
    ParseYaml { path: PathBuf, source: Box<serde_saphyr::Error> },
}

/// Error returned by [`write_modules_manifest`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum WriteModulesError {
    #[display("Failed to create directory {path:?}: {source}")]
    #[diagnostic(code(ERR_PNPM_MODULES_YAML_CREATE_DIR))]
    CreateDir { path: PathBuf, source: io::Error },

    #[display("Failed to serialize manifest: {_0}")]
    #[diagnostic(code(ERR_PNPM_MODULES_YAML_SERIALIZE_JSON))]
    SerializeJson(serde_json::Error),

    #[display("Failed to write {path:?}: {source}")]
    #[diagnostic(code(ERR_PNPM_MODULES_YAML_WRITE_IO))]
    WriteFile { path: PathBuf, source: io::Error },
}

/// Read `<modules_dir>/.modules.yaml` and return the normalized manifest.
///
/// Returns `Ok(None)` when the file does not exist or contains a YAML
/// `null` document.
///
/// Production callers turbofish [`Host`]: `read_modules_manifest::<Host>(dir)`.
/// The bounds list the minimal capabilities ([`FsReadToString`] +
/// [`Clock`]) so test fakes only need to implement the methods that are
/// actually called.
pub fn read_modules_manifest<Sys>(modules_dir: &Path) -> Result<Option<Modules>, ReadModulesError>
where
    Sys: FsReadToString + Clock,
{
    let manifest_path = modules_dir.join(MODULES_FILENAME);
    let content = match Sys::read_to_string(&manifest_path) {
        Ok(content) => content,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(ReadModulesError::ReadFile { path: manifest_path, source });
        }
    };
    let parsed: Option<Modules> =
        content.pipe_as_ref(serde_saphyr::from_str).map_err(|source| {
            ReadModulesError::ParseYaml { path: manifest_path.clone(), source: Box::new(source) }
        })?;
    let Some(mut manifest) = parsed else { return Ok(None) };
    apply_legacy_shamefully_hoist(&mut manifest);
    resolve_virtual_store_dir(&mut manifest, modules_dir);
    if manifest.pruned_at.is_empty() {
        manifest.pruned_at = httpdate::fmt_http_date(Sys::now());
    }
    if manifest.virtual_store_dir_max_length == 0 {
        manifest.virtual_store_dir_max_length = DEFAULT_VIRTUAL_STORE_DIR_MAX_LENGTH;
    }
    Ok(Some(manifest))
}

/// Reads the manifest into the lightweight [`ModulesLayout`] struct, skipping
/// large maps like `hoisted_dependencies` and `hoisted_locations`.
pub fn read_modules_layout<Sys>(
    modules_dir: &Path,
) -> Result<Option<ModulesLayout>, ReadModulesError>
where
    Sys: FsReadToString + Clock,
{
    let manifest_path = modules_dir.join(MODULES_FILENAME);
    let content = match Sys::read_to_string(&manifest_path) {
        Ok(content) => content,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(ReadModulesError::ReadFile { path: manifest_path, source });
        }
    };
    let parsed: Option<ModulesLayout> =
        content.pipe_as_ref(serde_saphyr::from_str).map_err(|source| {
            ReadModulesError::ParseYaml { path: manifest_path.clone(), source: Box::new(source) }
        })?;
    let Some(mut manifest) = parsed else { return Ok(None) };

    // Normalize legacy shamefully_hoist to public_hoist_pattern.
    if let Some(shamefully_hoist) = manifest.shamefully_hoist
        && manifest.public_hoist_pattern.is_none()
    {
        manifest.public_hoist_pattern =
            Some(if shamefully_hoist { vec!["*".to_string()] } else { Vec::new() });
    }

    let stored_path = Path::new(&manifest.virtual_store_dir);
    let resolved = match (manifest.virtual_store_dir.is_empty(), stored_path.is_absolute()) {
        (true, _) => modules_dir.join(".pnpm"),
        (false, true) => stored_path.to_path_buf(),
        (false, false) => lexical_normalize(&modules_dir.join(stored_path)),
    };
    manifest.virtual_store_dir = resolved.display().to_string();

    if manifest.pruned_at.is_empty() {
        manifest.pruned_at = httpdate::fmt_http_date(Sys::now());
    }
    if manifest.virtual_store_dir_max_length == 0 {
        manifest.virtual_store_dir_max_length = DEFAULT_VIRTUAL_STORE_DIR_MAX_LENGTH;
    }
    Ok(Some(manifest))
}

/// Write `manifest` to `<modules_dir>/.modules.yaml`, creating `modules_dir`
/// if it does not already exist.
///
/// Takes `manifest` by value because the body unconditionally rewrites
/// fields (sort `skipped`, drop legacy `hoistedAliases`, relativize
/// `virtualStoreDir`); making the caller hand over ownership keeps the
/// in-place mutation visible at the call site instead of forcing a hidden
/// `clone()` inside the function. Per the `CODE_STYLE_GUIDE` rule that
/// owned-vs-borrowed parameter choice should minimize copies.
///
/// Production callers turbofish [`Host`]: `write_modules_manifest::<Host>(dir, m)`.
/// Bounds are minimal: only [`FsCreateDirAll`] and [`FsWrite`] are required.
pub fn write_modules_manifest<Sys>(
    modules_dir: &Path,
    mut manifest: Modules,
) -> Result<(), WriteModulesError>
where
    Sys: FsCreateDirAll + FsWrite,
{
    manifest.skipped.sort();
    drop_legacy_hoisted_aliases_when_unreferenced(&mut manifest);
    // Junctions on Windows break when the project moves, so the absolute
    // path is intentionally preserved there.
    if !cfg!(windows) {
        rewrite_virtual_store_dir_relative(&mut manifest, modules_dir);
    }
    let serialized =
        serde_json::to_string_pretty(&manifest).map_err(WriteModulesError::SerializeJson)?;
    Sys::create_dir_all(modules_dir).map_err(|source| WriteModulesError::CreateDir {
        path: modules_dir.to_path_buf(),
        source,
    })?;
    let manifest_path = modules_dir.join(MODULES_FILENAME);
    Sys::write(&manifest_path, serialized.as_bytes())
        .map_err(|source| WriteModulesError::WriteFile { path: manifest_path, source })
}

/// When `virtualStoreDir` is missing, default to `modules_dir/.pnpm`. When
/// it is relative, resolve it against `modules_dir`.
fn resolve_virtual_store_dir(manifest: &mut Modules, modules_dir: &Path) {
    let stored_path = Path::new(&manifest.virtual_store_dir);
    let resolved = match (manifest.virtual_store_dir.is_empty(), stored_path.is_absolute()) {
        (true, _) => modules_dir.join(".pnpm"),
        (false, true) => stored_path.to_path_buf(),
        // Lexically normalize so the joined path collapses `..`
        // segments. Node's `path.join` does this; Rust's
        // [`PathBuf::join`] does not. Without normalization a stored
        // relative path like `../../Users/.../store/v11/links` joined
        // with `<workspace>/node_modules` round-trips as
        // `<workspace>/node_modules/../../Users/...`, which never byte-
        // matches the config's `effective_virtual_store_dir()` — and
        // [`crate::Install`]'s no-op short-circuit relies on that
        // equality to skip materialization on a clean install.
        (false, false) => lexical_normalize(&modules_dir.join(stored_path)),
    };
    manifest.virtual_store_dir = resolved.to_string_lossy().into_owned();
}

/// Store `virtualStoreDir` relative to `modules_dir`, falling back to the
/// original value when no relative form exists. This is the
/// `path.relative(modulesDir, virtualStoreDir)` of the manifest's
/// `virtualStoreDir`.
///
/// `pathdiff::diff_paths` is the Rust-side equivalent of Node's
/// `path.relative` (i.e. it produces `..` segments for non-descendant
/// targets); plain `Path::strip_prefix` would only handle descendants
/// and leave sibling/parent absolute paths untouched, which would
/// diverge from Node's `path.relative` output.
fn rewrite_virtual_store_dir_relative(manifest: &mut Modules, modules_dir: &Path) {
    let stored_path = Path::new(&manifest.virtual_store_dir);
    let relative =
        pathdiff::diff_paths(stored_path, modules_dir).unwrap_or_else(|| stored_path.to_path_buf());
    manifest.virtual_store_dir = relative.to_string_lossy().into_owned();
}

/// Translate the legacy `shamefullyHoist` and `hoistedAliases` fields into
/// the modern `publicHoistPattern` and `hoistedDependencies` shapes.
fn apply_legacy_shamefully_hoist(manifest: &mut Modules) {
    let Some(shamefully_hoist) = manifest.shamefully_hoist else {
        return;
    };
    let kind = if shamefully_hoist { HoistKind::Public } else { HoistKind::Private };
    match (&manifest.public_hoist_pattern, shamefully_hoist) {
        (None, false) => manifest.public_hoist_pattern = Some(Vec::new()),
        (None, true) => manifest.public_hoist_pattern = Some(vec!["*".to_string()]),
        (Some(_), _) => {}
    }
    if manifest.hoisted_dependencies.is_empty()
        && let Some(aliases_by_path) = &manifest.hoisted_aliases
    {
        manifest.hoisted_dependencies = aliases_by_path
            .iter()
            .map(|(dep_path, alias_names)| {
                let entry = alias_names.iter().cloned().zip(iter::repeat(kind)).collect();
                (dep_path.clone().into(), entry)
            })
            .collect();
    }
}

/// Drop the legacy `hoistedAliases` field on write when neither hoist
/// pattern is present.
fn drop_legacy_hoisted_aliases_when_unreferenced(manifest: &mut Modules) {
    if manifest.hoist_pattern.is_none() && manifest.public_hoist_pattern.is_none() {
        manifest.hoisted_aliases = None;
    }
}
