//! Read and write pnpm's `node_modules/.modules.yaml` manifest.
//!
//! Mirrors pnpm v11's `installing/modules-yaml` package. See upstream
//! <https://github.com/pnpm/pnpm/blob/1819226b51/installing/modules-yaml/src/index.ts>.
//!
//! The manifest is stored at `<modules_dir>/.modules.yaml`, where
//! `modules_dir` is the path of a `node_modules` directory. The on-disk
//! format is JSON (which YAML accepts), so reads use a YAML parser and
//! writes emit [`serde_json::to_string_pretty`] output to match pnpm exactly.

use derive_more::{Display, Error, From, Into};
use indexmap::IndexSet;
use pacquet_diagnostics::miette::{self, Diagnostic};
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
/// treat the file as an extraneous package. See upstream comment at
/// <https://github.com/pnpm/pnpm/blob/1819226b51/installing/modules-yaml/src/index.ts#L15-L17>.
pub const MODULES_FILENAME: &str = ".modules.yaml";

/// Default value for the `virtualStoreDirMaxLength` field.
///
/// Matches pnpm's fallback at
/// <https://github.com/pnpm/pnpm/blob/1819226b51/installing/modules-yaml/src/index.ts#L101-L103>.
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
/// Mirrors upstream's `new Date()` call at
/// <https://github.com/pnpm/pnpm/blob/1819226b51/installing/modules-yaml/src/index.ts#L98-L99>.
/// Decoupled from [`SystemTime::now`] so tests can fake the clock and
/// assert deterministic `prunedAt` values.
pub trait Clock {
    fn now() -> SystemTime;
}

/// Production implementation, backed by [`std::fs`] and [`SystemTime::now`].
pub struct RealApi;

impl FsReadToString for RealApi {
    #[inline]
    fn read_to_string(path: &Path) -> io::Result<String> {
        fs::read_to_string(path)
    }
}

impl FsCreateDirAll for RealApi {
    #[inline]
    fn create_dir_all(path: &Path) -> io::Result<()> {
        fs::create_dir_all(path)
    }
}

impl FsWrite for RealApi {
    #[inline]
    fn write(path: &Path, contents: &[u8]) -> io::Result<()> {
        fs::write(path, contents)
    }
}

impl Clock for RealApi {
    #[inline]
    fn now() -> SystemTime {
        SystemTime::now()
    }
}

/// Newtype wrapper around a dependency-path string. Mirrors upstream's
/// `DepPath` branded type at
/// <https://github.com/pnpm/pnpm/blob/1819226b51/core/types/src/misc.ts#L65>.
///
/// Upstream's `DepPath` is `string & { __brand: 'DepPath' }`, a branded
/// string. Every construction site uses an `as DepPath` cast — there are
/// no validating constructors anywhere in pnpm. The brand exists purely
/// to stop a plain `string` from being assigned where a `DepPath` is
/// expected at compile time. This Rust wrapper mirrors that contract: no
/// validation runs at construction, and `#[serde(transparent)]` makes the
/// wire format identical to `String` so a `DepPath` round-trips through
/// JSON / YAML the same way upstream's branded string does.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, From, Into,
)]
#[serde(transparent)]
pub struct DepPath(String);

impl DepPath {
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Typed view of a `node_modules/.modules.yaml` manifest.
///
/// Mirrors upstream's normalized [`Modules`](https://github.com/pnpm/pnpm/blob/1819226b51/installing/modules-yaml/src/index.ts#L46-L48)
/// type, which is [`ModulesRaw`](https://github.com/pnpm/pnpm/blob/1819226b51/installing/modules-yaml/src/index.ts#L23-L44)
/// with `ignoredBuilds` widened from `DepPath[]` (the on-disk shape)
/// to `Set<DepPath>` (the in-memory shape). Pacquet collapses the two
/// upstream types into a single struct: serde handles the
/// array↔[`IndexSet`] conversion at the [`Self::ignored_builds`]
/// field via [`IndexSet`]'s deduplicating `Deserialize` impl, so a
/// separate raw-shape type is not needed. `IndexSet` (insertion-ordered)
/// is chosen over `HashSet` / `BTreeSet` to match JavaScript `Set`'s
/// iteration semantics — the on-disk array order round-trips
/// byte-for-byte.
///
/// Every required-by-upstream field carries a `#[serde(default)]` so
/// legacy manifests written by older pnpm versions still deserialize;
/// the read path then fills in the modern shape from the legacy fields.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Modules {
    /// Legacy: the v5-era flat alias map, kept for read-side
    /// compatibility. Replaced by [`Self::hoisted_dependencies`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hoisted_aliases: Option<BTreeMap<DepPath, Vec<String>>>,

    /// Upstream's [`HoistedDependencies`](https://github.com/pnpm/pnpm/blob/1819226b51/core/types/src/misc.ts#L57)
    /// is `Record<DepPath | ProjectId, ...>`. Pacquet keeps the key as
    /// [`String`] because [`DepPath`] and `ProjectId` share the same
    /// underlying type with no validation, so the union cannot be
    /// disambiguated statically; the [`String`] type faithfully ports
    /// upstream's union.
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

    // TODO: upstream's `StrictModules` (which `writeModulesManifest`
    // takes) tightens this to a required `Registries`. Revisit when
    // the install-pipeline port supplies a producer that always
    // populates `default`.
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
    /// absent) and consulted by `lockfileToHoistedDepGraph`'s
    /// skip-fetch optimization to decide whether the package is
    /// already on disk. Mirrors upstream's optional
    /// `Record<string, string[]>` at
    /// <https://github.com/pnpm/pnpm/blob/94240bc046/installing/modules-yaml/src/index.ts#L43>.
    /// Pacquet's install pipeline does not populate this yet; the
    /// field is wired into the schema so a future hoisted-linker
    /// implementation can write it without changing the on-disk
    /// shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hoisted_locations: Option<BTreeMap<String, Vec<String>>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_builds: Option<BTreeMap<String, AllowBuildValue>>,
}

/// Which dependency groups the install pipeline included. Mirrors
/// upstream's `IncludedDependencies` at
/// <https://github.com/pnpm/pnpm/blob/1819226b51/installing/modules-yaml/src/index.ts#L19-L21>.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
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

/// Pinned identifier for the `node_modules` layout pacquet emits, mirroring
/// upstream's `LAYOUT_VERSION` constant at
/// <https://github.com/pnpm/pnpm/blob/1819226b51/core/constants/src/index.ts#L8>.
///
/// The unit type carries no data: its existence is the value. It serializes
/// as the integer `5` and deserializes only when the on-disk value is
/// exactly `5`. Any other version causes a deserialization error, mirroring
/// upstream's `checkCompatibility` reaction at
/// <https://github.com/pnpm/pnpm/blob/1819226b51/installing/deps-installer/src/install/checkCompatibility/index.ts#L18-L22>,
/// which throws `ModulesBreakingChangeError` for a missing or mismatched
/// `layoutVersion`. Wrapping this in [`Option`] on [`Modules`]
/// distinguishes "missing" (legacy, breaking change) from "present and
/// matching".
///
/// The `#[serde(try_from = "u32", into = "u32")]` proxy lets us reuse
/// serde's number deserializer, while the [`TryFrom`] impl owns the
/// "is this version supported" decision and returns
/// [`UnsupportedLayoutVersionError`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
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
    "Unsupported layout version {found}; this build of pacquet only supports layout version {}",
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
    #[diagnostic(code(pacquet_modules_yaml::read_io))]
    ReadFile { path: PathBuf, source: io::Error },

    #[display("Failed to parse {path:?}: {source}")]
    #[diagnostic(code(pacquet_modules_yaml::parse_yaml))]
    ParseYaml { path: PathBuf, source: Box<serde_saphyr::Error> },
}

/// Error returned by [`write_modules_manifest`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum WriteModulesError {
    #[display("Failed to create directory {path:?}: {source}")]
    #[diagnostic(code(pacquet_modules_yaml::create_dir))]
    CreateDir { path: PathBuf, source: io::Error },

    #[display("Failed to serialize manifest: {_0}")]
    #[diagnostic(code(pacquet_modules_yaml::serialize_json))]
    SerializeJson(serde_json::Error),

    #[display("Failed to write {path:?}: {source}")]
    #[diagnostic(code(pacquet_modules_yaml::write_io))]
    WriteFile { path: PathBuf, source: io::Error },
}

/// Read `<modules_dir>/.modules.yaml` and return the normalized manifest.
///
/// Returns `Ok(None)` when the file does not exist or contains a YAML
/// `null` document, matching upstream `readModules` at
/// <https://github.com/pnpm/pnpm/blob/1819226b51/installing/modules-yaml/src/index.ts#L50-L105>.
///
/// Production callers turbofish [`RealApi`]: `read_modules_manifest::<RealApi>(dir)`.
/// The bounds list the minimal capabilities ([`FsReadToString`] +
/// [`Clock`]) so test fakes only need to implement the methods that are
/// actually called.
pub fn read_modules_manifest<Api>(modules_dir: &Path) -> Result<Option<Modules>, ReadModulesError>
where
    Api: FsReadToString + Clock,
{
    let manifest_path = modules_dir.join(MODULES_FILENAME);
    let content = match Api::read_to_string(&manifest_path) {
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
        manifest.pruned_at = httpdate::fmt_http_date(Api::now());
    }
    if manifest.virtual_store_dir_max_length == 0 {
        manifest.virtual_store_dir_max_length = DEFAULT_VIRTUAL_STORE_DIR_MAX_LENGTH;
    }
    Ok(Some(manifest))
}

/// Write `manifest` to `<modules_dir>/.modules.yaml`, creating `modules_dir`
/// if it does not already exist.
///
/// Mirrors upstream `writeModules` at
/// <https://github.com/pnpm/pnpm/blob/1819226b51/installing/modules-yaml/src/index.ts#L111-L138>.
///
/// Takes `manifest` by value because the body unconditionally rewrites
/// fields (sort `skipped`, drop legacy `hoistedAliases`, relativize
/// `virtualStoreDir`); making the caller hand over ownership keeps the
/// in-place mutation visible at the call site instead of forcing a hidden
/// `clone()` inside the function. Per the CODE_STYLE_GUIDE rule that
/// owned-vs-borrowed parameter choice should minimize copies.
///
/// Production callers turbofish [`RealApi`]: `write_modules_manifest::<RealApi>(dir, m)`.
/// Bounds are minimal: only [`FsCreateDirAll`] and [`FsWrite`] are required.
pub fn write_modules_manifest<Api>(
    modules_dir: &Path,
    mut manifest: Modules,
) -> Result<(), WriteModulesError>
where
    Api: FsCreateDirAll + FsWrite,
{
    manifest.skipped.sort();
    drop_legacy_hoisted_aliases_when_unreferenced(&mut manifest);
    // Junctions on Windows break when the project moves, so the absolute
    // path is intentionally preserved there. See upstream
    // <https://github.com/pnpm/pnpm/blob/1819226b51/installing/modules-yaml/src/index.ts#L129-L135>.
    if !cfg!(windows) {
        rewrite_virtual_store_dir_relative(&mut manifest, modules_dir);
    }
    let serialized =
        serde_json::to_string_pretty(&manifest).map_err(WriteModulesError::SerializeJson)?;
    Api::create_dir_all(modules_dir).map_err(|source| WriteModulesError::CreateDir {
        path: modules_dir.to_path_buf(),
        source,
    })?;
    let manifest_path = modules_dir.join(MODULES_FILENAME);
    Api::write(&manifest_path, serialized.as_bytes())
        .map_err(|source| WriteModulesError::WriteFile { path: manifest_path, source })
}

/// When `virtualStoreDir` is missing, default to `modules_dir/.pnpm`. When
/// it is relative, resolve it against `modules_dir`. Mirrors upstream's
/// resolution at
/// <https://github.com/pnpm/pnpm/blob/1819226b51/installing/modules-yaml/src/index.ts#L66-L70>.
fn resolve_virtual_store_dir(manifest: &mut Modules, modules_dir: &Path) {
    let stored_path = Path::new(&manifest.virtual_store_dir);
    let resolved = match (manifest.virtual_store_dir.is_empty(), stored_path.is_absolute()) {
        (true, _) => modules_dir.join(".pnpm"),
        (false, true) => stored_path.to_path_buf(),
        (false, false) => modules_dir.join(stored_path),
    };
    manifest.virtual_store_dir = resolved.to_string_lossy().into_owned();
}

/// Store `virtualStoreDir` relative to `modules_dir`, falling back to the
/// original value when no relative form exists. Mirrors upstream's
/// `path.relative(modulesDir, saveModules.virtualStoreDir)` at
/// <https://github.com/pnpm/pnpm/blob/1819226b51/installing/modules-yaml/src/index.ts#L132-L135>.
///
/// `pathdiff::diff_paths` is the Rust-side equivalent of Node's
/// `path.relative` (i.e. it produces `..` segments for non-descendant
/// targets); plain `Path::strip_prefix` would only handle descendants
/// and leave sibling/parent absolute paths untouched, which would
/// diverge from upstream's `path.relative` output.
fn rewrite_virtual_store_dir_relative(manifest: &mut Modules, modules_dir: &Path) {
    let stored_path = Path::new(&manifest.virtual_store_dir);
    let relative =
        pathdiff::diff_paths(stored_path, modules_dir).unwrap_or_else(|| stored_path.to_path_buf());
    manifest.virtual_store_dir = relative.to_string_lossy().into_owned();
}

/// Translate the legacy `shamefullyHoist` and `hoistedAliases` fields into
/// the modern `publicHoistPattern` and `hoistedDependencies` shapes. Mirrors
/// upstream's translation block at
/// <https://github.com/pnpm/pnpm/blob/1819226b51/installing/modules-yaml/src/index.ts#L71-L97>.
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
/// pattern is present, mirroring upstream's cleanup at
/// <https://github.com/pnpm/pnpm/blob/1819226b51/installing/modules-yaml/src/index.ts#L126-L128>.
fn drop_legacy_hoisted_aliases_when_unreferenced(manifest: &mut Modules) {
    if manifest.hoist_pattern.is_none() && manifest.public_hoist_pattern.is_none() {
        manifest.hoisted_aliases = None;
    }
}
