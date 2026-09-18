use super::{AuditLevel, BTreeMap, Deserialize, Deserializer, HashMap, IndexMap, overlay_some};

/// The value of an `allowBuilds` entry.
///
/// pnpm scaffolds an entry per ignored build with the placeholder string
/// `set this to true or false` for the user to edit, so the file it wrote
/// itself must stay loadable. Only [`AllowBuild::Decided`] entries reach
/// [`Config::allow_builds`](crate::settings::Config::allow_builds); an undecided one leaves the package under the
/// default-deny policy, exactly as pnpm's `createAllowBuildFunction`
/// (which matches on `true`/`false` and ignores anything else) does.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(untagged)]
pub enum AllowBuild {
    Decided(bool),
    Undecided(String),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(untagged)]
pub enum PnpmfileSetting {
    Single(String),
    Multiple(Vec<String>),
}

impl AllowBuild {
    /// The policy this entry resolves to, or `None` while it is still an
    /// unedited placeholder.
    #[must_use]
    pub fn decided(&self) -> Option<bool> {
        match self {
            AllowBuild::Decided(allowed) => Some(*allowed),
            AllowBuild::Undecided(_) => None,
        }
    }
}

/// Reduce a parsed `allowBuilds` map to the entries that drive the build
/// policy, dropping the ones still awaiting a decision.
#[must_use]
pub fn decided_allow_builds(allow_builds: HashMap<String, AllowBuild>) -> HashMap<String, bool> {
    allow_builds
        .into_iter()
        .filter_map(|(pkg, value)| Some((pkg, value.decided()?)))
        .collect()
}

/// Organization-owned dependency build artifacts eligible for this workspace.
///
/// `org` and `packages` default to empty because one section is
/// assembled from several sources: the repository names the eligible
/// organization and packages while the machine supplies the trust root. The
/// feature applies only once both halves are present.
///
/// Only `org` and `packages` may come from a repository. Every other
/// field describes the act of signing and travels with the machine: loading a
/// `pnpm-workspace.yaml` that sets one fails with
/// [`LoadWorkspaceYamlError::WorkspaceRemoteSideEffectsTrust`](crate::workspace_yaml::error::LoadWorkspaceYamlError::WorkspaceRemoteSideEffectsTrust), leaving the
/// global config yaml and the environment.
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "The fields mirror a pnpm-workspace.yaml configuration section."
    )
)]
pub struct RemoteSideEffectsCacheSettings {
    /// `org` is what pnpr calls this namespace in its own configuration and
    /// what its endpoints are built from.
    pub org: String,
    /// The alternative spelling of [`Self::org`]. A non-empty [`Self::org`]
    /// wins over this field.
    ///
    /// A separate field rather than a serde alias: an alias makes a file
    /// carrying both keys a duplicate-field parse error, where every other
    /// pair of spellings here resolves to the canonical one.
    pub organization: String,
    pub packages: Vec<String>,
    /// Publish the lifecycle-script diff of every eligible package that is built.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publish: Option<bool>,
    /// Identifies which of the consumer's trusted keys signed a published artifact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builder_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub architecture_baseline: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_env: Option<BTreeMap<String, String>>,
    /// Base64-encoded P-256 `SubjectPublicKeyInfo` DER, keyed by key id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trusted_keys: Option<BTreeMap<String, String>>,
    /// Base64-encoded PKCS#8 P-256 private key used to sign published artifacts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
}

/// A program pnpm downloads through a base URL, which is what a mirror
/// can replace.
///
/// Deno and Yarn are absent on purpose: both read the GitHub API for
/// their release metadata and take asset URLs out of the response, so a
/// base URL cannot stand in for either. Naming one would promise
/// something pnpm cannot do, and a closed set says so where the
/// configuration is written rather than after it is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Tool {
    Node,
    Bun,
    Python,
}

/// What pnpm is told about one tool it downloads, keyed by the tool
/// under `tools`.
///
/// A tool here is a program pnpm fetches to run something with: a
/// JavaScript runtime, a Python interpreter, another package manager.
/// Where the packages of an ecosystem come from is a separate question,
/// answered by `registry`, `python.indexUrl` and `cargo.indexUrl`.
///
/// Read from the global `config.yaml` and `PNPM_CONFIG_TOOLS` only. A
/// `pnpm-workspace.yaml` that names one is ignored, because naming a
/// mirror chooses which program runs on the machine of everyone who
/// clones the repository.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct ToolSettings {
    /// Where this tool's builds are downloaded from, in place of the
    /// project that publishes them, as the base URL its own layout hangs
    /// off. A tool that publishes several lines of builds has the line
    /// below this base, the way its own tree lays them out.
    pub mirror: Option<String>,
    /// Where one line of this tool's builds comes from, for a tool that
    /// publishes more than one and a line that does not come from the
    /// same place as the rest. An entry here answers for the channel it
    /// names; every other channel is left to [`Self::mirror`].
    ///
    /// Only Node.js publishes channels of the tools pnpm downloads, so
    /// naming them for another is refused rather than left to do
    /// nothing. [`Tool::has_channels`] is what decides.
    pub channels: Option<BTreeMap<String, String>>,
}

impl Tool {
    /// Whether this tool publishes more than one line of builds.
    #[must_use]
    pub fn has_channels(self) -> bool {
        match self {
            Self::Node => true,
            Self::Bun | Self::Python => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct CargoSettings {
    pub enabled: bool,
    pub index_url: String,
}

impl Default for CargoSettings {
    fn default() -> Self {
        Self { enabled: false, index_url: "https://index.crates.io".to_string() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "PythonSettings matches the public python configuration table in pnpm-workspace.yaml."
    )
)]
pub struct PythonSettings {
    pub enabled: bool,
    /// The interpreter to install every Python project with. `None` lets
    /// pnpm choose one the project accepts.
    pub executable: Option<String>,
    pub index_url: String,
    pub extra_index_urls: Vec<String>,
    pub overrides: Vec<String>,
    pub constraints: Vec<String>,
    pub extras: Vec<String>,
    pub groups: Vec<String>,
    /// The Python versions `pylock.toml` is resolved for, each a minor
    /// version such as `3.12` or a full one such as `3.12.7`. Empty locks
    /// for the version of the interpreter the install runs on.
    pub python_versions: Vec<String>,
}

impl Default for PythonSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            executable: None,
            index_url: "https://pypi.org/simple/".to_string(),
            extra_index_urls: Vec::new(),
            overrides: Vec::new(),
            constraints: Vec::new(),
            extras: Vec::new(),
            groups: vec!["dev".to_string()],
            python_versions: Vec::new(),
        }
    }
}

/// The releases of [python-build-standalone], the interpreter builds uv,
/// rye, hatch and mise install too.
///
/// [python-build-standalone]: https://github.com/astral-sh/python-build-standalone
pub const DEFAULT_PYTHON_DOWNLOAD_URL: &str =
    "https://github.com/astral-sh/python-build-standalone/releases";

/// `sideEffectsCache` as written: either a bare boolean, or the declaration
/// carrying all three parts.
#[derive(Debug, PartialEq, serde::Serialize, Deserialize)]
#[serde(untagged)]
pub enum SideEffectsCacheSetting {
    Enabled(bool),
    /// Boxed because the shorthand is one byte and this is not, and an
    /// `Option<SideEffectsCacheSetting>` sits in a struct built for every
    /// workspace file read.
    Settings(Box<SideEffectsCacheSettings>),
}

/// Where a dependency's build output may be reused from: this machine, and —
/// through [`Self::remote`] — other machines in the same organization.
#[derive(Debug, Default, PartialEq, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SideEffectsCacheSettings {
    /// Restore a package's build from the cache when one is present.
    pub read: Option<bool>,
    /// Save a package's build output to the cache.
    pub write: Option<bool>,
    pub remote: Option<RemoteSideEffectsCacheSettings>,
}

impl RemoteSideEffectsCacheSettings {
    /// Overlay the fields `other` sets onto `self`, leaving the rest alone.
    ///
    /// A workspace declares eligibility while the machine holds the signing
    /// trust root, so the two sources contribute different fields of one
    /// section and the later one must not drop what the earlier one set.
    pub(crate) fn overlay(&mut self, other: Self) {
        let Self {
            org,
            organization,
            packages,
            publish,
            key_id,
            builder_id,
            image_digest,
            architecture_baseline,
            build_env,
            trusted_keys,
            private_key,
        } = other;
        // Resolved as the section is layered rather than at each read, so
        // that `.org` is the only spelling anything downstream has to know.
        let org = if org.is_empty() { organization } else { org };
        if !org.is_empty() {
            self.org = org;
        }
        if !packages.is_empty() {
            self.packages = packages;
        }
        overlay_some(&mut self.publish, publish);
        overlay_some(&mut self.key_id, key_id);
        overlay_some(&mut self.builder_id, builder_id);
        overlay_some(&mut self.image_digest, image_digest);
        overlay_some(&mut self.architecture_baseline, architecture_baseline);
        overlay_some(&mut self.build_env, build_env);
        overlay_some(&mut self.trusted_keys, trusted_keys);
        overlay_some(&mut self.private_key, private_key);
    }
}

/// `audit` entry: settings that tune `pnpm audit`. Supersedes the
/// deprecated top-level `auditLevel` and the `auditConfig` entry.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AuditSettings {
    /// Minimum vulnerability severity `pnpm audit` reports on.
    /// Supersedes the deprecated top-level `auditLevel`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<AuditLevel>,

    /// GHSA IDs `pnpm audit` ignores. Supersedes the deprecated
    /// [`AuditConfig::ignore_ghsas`](crate::setting_types::AuditConfig::ignore_ghsas).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignore: Option<Vec<String>>,

    /// When `true`, `pnpm audit --fix` removes entries from the ignore
    /// list that no longer appear in the audit report, so a re-introduced
    /// vulnerability under the same GHSA ID gets re-evaluated instead of
    /// staying silently suppressed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignore_prune: Option<bool>,
}

/// `update` entry: settings that tune `pnpm update` (and `pnpm
/// outdated`, which previews it). Supersedes the deprecated
/// `updateConfig`.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateSettings {
    /// `ignoreDeps`: dependency-name patterns `pnpm update` and `pnpm
    /// outdated` skip. Glob/negation patterns. Equivalent to the
    /// deprecated [`UpdateConfig::ignore_dependencies`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignore_deps: Option<Vec<String>>,

    /// `changeset`: generate a changeset for the updated production
    /// dependencies by default, as if `pnpm update` were run with
    /// `--changeset`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changeset: Option<bool>,

    /// Whether `pnpm outdated` and `pnpm update` should also look at
    /// the GitHub Actions referenced by the workflow files. Opt-in:
    /// neither command reads them unless this is set to `true` or
    /// `--include-github-actions` is passed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub github_actions: Option<bool>,

    /// `githubActionsServer`: the base URL of the GitHub server that
    /// hosts the repositories of the GitHub Actions referenced by the
    /// workflow files (for example, a GitHub Enterprise Server). When
    /// not set, the `GITHUB_SERVER_URL` environment variable is used,
    /// falling back to <https://github.com>.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub github_actions_server: Option<String>,
}

/// One task's entry in the `tasks` section. A task name is a script name:
/// `pnpm -r run <name>` runs the task named `<name>` in every selected
/// project.
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase", default)]
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "The fields mirror a pnpm-workspace.yaml configuration section."
    )
)]
pub struct TaskSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<i64>,

    #[serde(skip)]
    pub(super) invalid_concurrency: Option<serde_json::Value>,

    /// The concurrency group the task counts against: at most
    /// `concurrencyGroups.<group>` tasks of the group run at once on the
    /// machine, across every pnpm process. See
    /// [`Config::concurrency_groups`](crate::settings::Config::concurrency_groups).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concurrency_group: Option<String>,

    /// The tasks that must complete before this one may start. A `^name`
    /// entry names the task in each of the project's workspace
    /// dependencies; a bare `name` entry names the task in the same
    /// project.
    ///
    /// A task with no declaration behaves as `dependsOn: ['^<its own
    /// name>']`. An entry with `dependsOn` omitted declares an empty
    /// dependency list — the task depends on nothing and may start
    /// immediately.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depends_on: Option<Vec<String>>,

    /// The globs, relative to the project directory, naming the files the
    /// task produces. Declaring `outputs` (even as `[]`, the positive
    /// assertion that the task produces no files) is what makes a task
    /// cacheable by `pnpm pipeline`; a task without the key runs normally.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outputs: Option<Vec<String>>,

    /// The globs, relative to the project directory, narrowing the task's
    /// cache-key inputs. Absent, the inputs are every tracked (and
    /// untracked, unignored) file of the project; a `+`-prefixed entry adds
    /// to that default instead of replacing it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inputs: Option<Vec<String>>,

    /// Environment variable names whose values participate in the task's
    /// cache key. Values are hashed into the key, never recorded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<Vec<String>>,

    /// `false` opts a task with declared `outputs` out of the cache.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<bool>,

    /// Opt into local Cargo state snapshots for this project-relative target
    /// directory. Pipeline always executes the task and sets Cargo's target
    /// and build directories to this path. Overrides output-cache restoration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cargo_target_dir: Option<String>,

    /// Fields this version of pnpm does not read, kept so validation can
    /// reject a typo instead of silently ignoring it.
    #[serde(flatten, skip_serializing_if = "IndexMap::is_empty")]
    pub unknown: IndexMap<String, serde_json::Value>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "The fields mirror a pnpm-workspace.yaml configuration section."
    )
)]
struct RawTaskSettings {
    concurrency: Option<serde_json::Value>,
    concurrency_group: Option<String>,
    depends_on: Option<Vec<String>>,
    outputs: Option<Vec<String>>,
    inputs: Option<Vec<String>>,
    env: Option<Vec<String>>,
    cache: Option<bool>,
    cargo_target_dir: Option<String>,
    #[serde(flatten)]
    unknown: IndexMap<String, serde_json::Value>,
}

impl<'de> Deserialize<'de> for TaskSettings {
    fn deserialize<De: Deserializer<'de>>(deserializer: De) -> Result<Self, De::Error> {
        let raw = RawTaskSettings::deserialize(deserializer)?;
        let concurrency = raw.concurrency.as_ref().and_then(serde_json::Value::as_i64);
        let invalid_concurrency = raw.concurrency.filter(|value| value.as_i64().is_none());
        Ok(Self {
            concurrency,
            invalid_concurrency,
            concurrency_group: raw.concurrency_group,
            depends_on: raw.depends_on,
            outputs: raw.outputs,
            inputs: raw.inputs,
            env: raw.env,
            cache: raw.cache,
            cargo_target_dir: raw.cargo_target_dir,
            unknown: raw.unknown,
        })
    }
}

/// `updateConfig` entry: settings that tune `pnpm update`.
///
/// Deprecated in favor of [`UpdateSettings`], kept for backward
/// compatibility until the next major version.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateConfig {
    /// Generate changesets for production dependency changes by default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changeset: Option<bool>,

    /// Dependency-name patterns `pnpm update` skips. Glob/negation
    /// patterns.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignore_dependencies: Option<Vec<String>>,

    /// Whether `pnpm outdated` and `pnpm update` should also look at
    /// the GitHub Actions referenced by the workflow files. Opt-in:
    /// neither command reads them unless this is set to `true` or
    /// `--include-github-actions` is passed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub github_actions: Option<bool>,

    /// The base URL of the GitHub server that hosts the repositories of
    /// the GitHub Actions referenced by the workflow files (for example,
    /// a GitHub Enterprise Server). When not set, the
    /// `GITHUB_SERVER_URL` environment variable is used, falling back to
    /// <https://github.com>.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub github_actions_server: Option<String>,
}

/// `peerDependencyRules` entry: customizations applied when reporting
/// peer-dependency issues.
///
/// - `ignoreMissing` / `allowAny` are glob/negation pattern lists
///   (matched against the peer package name).
/// - `allowedVersions` maps a peer selector (`name`, or the override
///   form `parent>name` / `parent@range>name`) to an extra semver range
///   that should be accepted.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PeerDependencyRules {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignore_missing: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_any: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_versions: Option<BTreeMap<String, String>>,
}

/// One `packageExtensions` entry: a subset of a manifest's dependency
/// groups, merged onto every matching manifest at install time. The
/// fields are `dependencies`, `optionalDependencies`,
/// `peerDependencies`, and `peerDependenciesMeta`.
///
/// Read directly from yaml — no validation here beyond serde's shape
/// check. The hook
/// (`pnpm_package_manager::PackageExtender`) merges these onto
/// manifests, with the manifest's own fields taking precedence on
/// conflict so the extension never overwrites a value the package
/// already declared.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PackageExtension {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optional_dependencies: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer_dependencies: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer_dependencies_meta: Option<BTreeMap<String, PeerDependencyMeta>>,
}

/// `peerDependenciesMeta` entry shape: a single `optional` flag today.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PeerDependencyMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optional: Option<bool>,
}

/// Refuse `channels` for a tool that publishes one line of builds, where
/// it would otherwise sit in the configuration doing nothing.
///
/// Checked as the value is read so that every source is covered: the
/// workspace file, the global `config.yaml` and `PNPM_CONFIG_TOOLS` all
/// arrive through serde.
pub(crate) fn deserialize_tools<'de, Deser: Deserializer<'de>>(
    deserializer: Deser,
) -> Result<Option<BTreeMap<Tool, ToolSettings>>, Deser::Error> {
    use serde::de::Error as _;
    let tools = Option::<BTreeMap<Tool, ToolSettings>>::deserialize(deserializer)?;
    for (tool, settings) in tools.iter().flatten() {
        if settings.channels.is_some() && !tool.has_channels() {
            return Err(Deser::Error::custom(format!(
                "{tool:?} publishes one line of builds, so it has no channels to name",
            )));
        }
    }
    Ok(tools)
}
