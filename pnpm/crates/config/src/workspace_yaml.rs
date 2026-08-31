use crate::{
    AuditConfig, AuditLevel, CatalogMode, Config, HoistingLimits, InitType, LinkWorkspacePackages,
    NodeLinker, NodePackageMapType, PackageImportMethod, PmOnFail, ResolutionMode, RuntimeOnFail,
    SaveWorkspaceProtocol, ScriptsPrependNodePath, TrustPolicy, VerifyDepsBeforeRun,
    VirtualStoreType,
    api::{EnvVar, GetHomeDir},
    config_types::is_config_file_key,
    known_settings::{SCHEMA_DIRECTIVE_KEY, annotate_unknown_setting, is_known_setting_key},
    naming_cases::{is_camel_case, to_camel_case, to_kebab_case},
    proxy_keys::{ProxyKeys, ProxyValue},
    refused_keys::{is_refused_by_a_project_manifest, where_refused_key_belongs},
    resolve_child_concurrency,
};
use derive_more::{Display, Error};
use indexmap::IndexMap;
use miette::Diagnostic;
use pipe_trait::Pipe;
use pnpm_env_replace::env_replace_lossy;
use pnpm_network::redact_and_sanitize;
use pnpm_package_is_installable::SupportedArchitectures;
use pnpm_store_dir::StoreDir;
use pnpm_workspace_state::ConfigDependency;
use registries::RegistryEntry;
use serde::{Deserialize, Deserializer, de::IgnoredAny};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};

/// `serde` helper for fields that need to distinguish "missing key"
/// from "explicit null" in YAML / JSON.
///
/// Stand-alone helper rather than reaching for `serde_with` (not in
/// the workspace deps) — the body is one line.
fn deserialize_double_option<'de, Value, De>(
    deserializer: De,
) -> Result<Option<Option<Value>>, De::Error>
where
    Value: Deserialize<'de>,
    De: Deserializer<'de>,
{
    Option::<Value>::deserialize(deserializer).map(Some)
}

/// Whether the authority of `url` carries a `user:pass@` prefix. The authority
/// ends at the first `/`, `?`, or `#`, so a later `@` in the path is not one.
///
/// Both the full form and the scheme-less `//host/` form count. The latter is
/// the shape `.npmrc` scopes settings with, so it is the one a user is most
/// likely to reach for here.
fn registry_url_has_userinfo(url: &str) -> bool {
    userinfo_end(url).is_some()
}

/// The offset just past the `user:pass@` of `url`, or [`None`] when its
/// authority carries none. Splitting it out keeps the detection and the
/// redaction below agreeing on what the authority is.
fn userinfo_end(url: &str) -> Option<usize> {
    let authority_start = authority_start_of(url)?;
    let authority = &url[authority_start..];
    let authority_end = authority.find(['/', '?', '#']).unwrap_or(authority.len());
    authority[..authority_end].rfind('@').map(|at| authority_start + at + 1)
}

/// Where the authority of `url` begins, or [`None`] if it has none.
///
/// The scheme is anchored at the start rather than found by searching for the
/// first `://`: a `://` inside the path (`//host/a://b`) would otherwise be
/// taken for the separator, and the real authority — credentials and all —
/// would go unexamined.
fn authority_start_of(url: &str) -> Option<usize> {
    if let Some(scheme_end) = url.find("://") {
        let scheme = &url[..scheme_end];
        let mut chars = scheme.chars();
        let starts_with_letter = chars.next().is_some_and(|first| first.is_ascii_alphabetic());
        let rest_is_scheme = chars.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '.' | '-')
        });
        if starts_with_letter && rest_is_scheme {
            return Some(scheme_end + "://".len());
        }
    }
    url.starts_with("//").then_some("//".len())
}

/// `url` with any `user:pass@` removed, safe to put in a message.
///
/// [`redact_and_sanitize`] only recognizes an authority after a `://`, and
/// deliberately so: it runs over arbitrary prose, where a bare `//` is more
/// often a comment or a path than a URL. Here the string is known to be a
/// registry URL, so the scheme-less `//host/` form can be handled too.
fn redact_registry_url(url: &str) -> String {
    match (authority_start_of(url), userinfo_end(url)) {
        (Some(authority_start), Some(userinfo_end)) => {
            redact_and_sanitize(&format!("{}{}", &url[..authority_start], &url[userinfo_end..]))
        }
        _ => redact_and_sanitize(url),
    }
}

/// The value of an `allowBuilds` entry.
///
/// pnpm scaffolds an entry per ignored build with the placeholder string
/// `set this to true or false` for the user to edit, so the file it wrote
/// itself must stay loadable. Only [`AllowBuild::Decided`] entries reach
/// [`Config::allow_builds`]; an undecided one leaves the package under the
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
    allow_builds.into_iter().filter_map(|(pkg, value)| Some((pkg, value.decided()?))).collect()
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
/// [`LoadWorkspaceYamlError::WorkspaceRemoteSideEffectsTrust`], leaving the
/// global config yaml and the environment.
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
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
        if publish.is_some() {
            self.publish = publish;
        }
        if key_id.is_some() {
            self.key_id = key_id;
        }
        if builder_id.is_some() {
            self.builder_id = builder_id;
        }
        if image_digest.is_some() {
            self.image_digest = image_digest;
        }
        if architecture_baseline.is_some() {
            self.architecture_baseline = architecture_baseline;
        }
        if build_env.is_some() {
            self.build_env = build_env;
        }
        if trusted_keys.is_some() {
            self.trusted_keys = trusted_keys;
        }
        if private_key.is_some() {
            self.private_key = private_key;
        }
    }
}

/// Settings readable from `pnpm-workspace.yaml`.
///
/// pnpm 10+ moved the bulk of its configuration (`storeDir`, `registry`,
/// `lockfile`, ...) out of `.npmrc` into `pnpm-workspace.yaml`, using
/// camelCase keys. Pacquet needs to honour these overrides so a real
/// pnpm-11-style project — where `.npmrc` may not even contain the
/// settings — works out of the box.
///
/// Every field is `Option` because the yaml is strictly additive on top of
/// [`Config`]: anything left unset falls through to whatever `.npmrc` provided
/// (or the hard-coded default).
///
/// See <https://pnpm.io/settings> for the canonical key list.
/// Workspace-structural keys (`packages`, `catalog`, `catalogs`, the build
/// allowlists) are carried only for `pnpm config get` / `list` — see
/// [`Self::packages`]. Anything else that is not a field is silently
/// ignored — serde drops it since the struct doesn't use
/// `deny_unknown_fields`.
///
/// pnpm v11 also reads `patchedDependencies` (and the other install
/// settings such as `allowBuilds`) from this file rather than from
/// `package.json`'s `pnpm` field, resolving those settings against the
/// workspace dir.
#[derive(Debug, Default, PartialEq, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WorkspaceSettings {
    pub bail: Option<bool>,
    pub ci: Option<bool>,
    pub update_notifier: Option<bool>,
    pub color: Option<crate::ColorMode>,
    pub embed_readme: Option<bool>,
    pub ignore_workspace_root_check: Option<bool>,
    pub optional: Option<bool>,
    pub package_lock: Option<bool>,
    pub pending: Option<bool>,
    pub recursive_install: Option<bool>,
    pub reverse: Option<bool>,
    pub stream: Option<bool>,
    pub aggregate_output: Option<bool>,
    pub reporter_hide_prefix: Option<bool>,
    pub use_stderr: Option<bool>,
    pub ignore_workspace: Option<bool>,
    pub shell_emulator: Option<bool>,
    pub skip_manifest_obfuscation: Option<bool>,
    pub sort: Option<bool>,
    pub use_beta_cli: Option<bool>,
    pub hoist: Option<bool>,

    /// Tri-state `hoistPattern` — see `deserialize_double_option`.
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub hoist_pattern: Option<Option<Vec<String>>>,

    /// Tri-state `publicHoistPattern`. Same semantics as
    /// [`Self::hoist_pattern`].
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub public_hoist_pattern: Option<Option<Vec<String>>>,
    pub shamefully_hoist: Option<bool>,
    pub store_dir: Option<String>,
    pub state_dir: Option<String>,
    pub modules_dir: Option<String>,
    pub node_linker: Option<NodeLinker>,
    pub node_experimental_package_map: Option<bool>,
    pub node_package_map_type: Option<NodePackageMapType>,
    pub symlink: Option<bool>,
    pub virtual_store_dir: Option<String>,
    /// `virtualStoreType` from `pnpm-workspace.yaml`. See
    /// [`crate::VirtualStoreType`], and
    /// [`Config::enable_global_virtual_store`] for the default.
    pub virtual_store_type: Option<VirtualStoreType>,
    /// `enableGlobalVirtualStore`, the boolean spelling of
    /// [`Self::virtual_store_type`]. A file may carry either or both; the
    /// canonical key wins.
    pub enable_global_virtual_store: Option<bool>,
    /// `virtualStoreOnly` from `pnpm-workspace.yaml`. See
    /// [`Config::virtual_store_only`].
    pub virtual_store_only: Option<bool>,
    /// `globalShims` from `pnpm-workspace.yaml` or the global
    /// `config.yaml`. One layer of the record; merged key-wise into
    /// [`Config::global_shims`] rather than assigned
    /// wholesale. See [`crate::GlobalShims`].
    pub global_shims: Option<crate::GlobalShimsSetting>,
    /// `enableModulesDir` from `pnpm-workspace.yaml`. See
    /// [`Config::enable_modules_dir`].
    pub enable_modules_dir: Option<bool>,
    /// `globalVirtualStoreDir` from `pnpm-workspace.yaml`. Resolved
    /// against the workspace dir like the other path-valued fields.
    /// When set, overrides the derived `<store_dir>/links` path.
    pub global_virtual_store_dir: Option<String>,
    /// `globalDir` from the global `config.yaml` or the environment. A
    /// relative value resolves against the directory pnpm runs in, which
    /// is where pnpm itself resolves it. See [`Config::global_dir`].
    ///
    /// No repo-committed file may set it — see [`crate::refused_keys`].
    pub global_dir: Option<String>,
    /// `globalBinDir` from the global `config.yaml` or the environment. A
    /// relative value resolves against the directory pnpm runs in, which
    /// is where pnpm itself resolves it. See [`Config::global_bin_dir`].
    ///
    /// No repo-committed file may set it — see [`crate::refused_keys`].
    pub global_bin_dir: Option<String>,
    pub package_import_method: Option<PackageImportMethod>,
    pub modules_cache_max_age: Option<u64>,
    pub virtual_store_dir_max_length: Option<u64>,
    pub peers_suffix_max_length: Option<u64>,
    pub lockfile: Option<bool>,
    /// `lockfileDir` from `pnpm-workspace.yaml` or the global
    /// `config.yaml`. Resolved against the workspace dir like the other
    /// path-valued fields. See [`Config::lockfile_dir`].
    pub lockfile_dir: Option<String>,
    pub prefer_frozen_lockfile: Option<bool>,

    /// `frozenLockfile` from `pnpm-workspace.yaml`. Unset by default:
    /// see [`Config::frozen_lockfile`].
    pub frozen_lockfile: Option<bool>,
    pub deploy_all_files: Option<bool>,
    pub force_legacy_deploy: Option<bool>,
    pub shared_workspace_lockfile: Option<bool>,
    pub git_branch_lockfile: Option<bool>,
    pub merge_git_branch_lockfiles: Option<bool>,
    pub merge_git_branch_lockfiles_branch_pattern: Option<Vec<String>>,
    pub offline: Option<bool>,
    pub prefer_offline: Option<bool>,
    pub lockfile_include_tarball_url: Option<bool>,
    pub registry: Option<String>,
    pub scope: Option<String>,
    /// The registries the project declares. Keyed by registry URL, with the
    /// routes to each registry inside its entry; a map of plain strings is the
    /// older `<scope>: <url>` shape and is read as one.
    pub registries: Option<BTreeMap<String, RegistryEntry>>,
    pub pnpr_server: Option<String>,
    pub remote_side_effects_cache: Option<RemoteSideEffectsCacheSettings>,
    pub https_proxy: Option<String>,
    pub http_proxy: Option<String>,
    pub no_proxy: Option<serde_json::Value>,
    pub proxy: Option<String>,
    pub noproxy: Option<serde_json::Value>,

    /// User-defined named-registry aliases. Outer key is the alias
    /// name (`gh`, `work`, ...); inner string is the registry URL the
    /// alias resolves against. Merged on top of pnpm's built-in
    /// defaults at resolver construction.
    ///
    /// Deprecated in favor of the `prefix` field of a
    /// [`crate::RegistryDeclaration`],
    /// and only read for the prefixes `registries` does not declare.
    pub named_registries: Option<BTreeMap<String, String>>,

    /// Structured registry auth (`_auth`). Honored **only** from the global
    /// pnpm `config.yaml` (read via `NpmrcAuth::from_json_sources`, not
    /// applied in [`Self::apply_to`]) — never a project file, so repo config
    /// can't supply credentials. A raw [`serde_json::Value`] so the auth
    /// parser is the single validator of its shape.
    #[serde(rename = "_auth")]
    pub auth: Option<serde_json::Value>,

    pub auto_install_peers: Option<bool>,
    pub auto_install_peers_from_highest_match: Option<bool>,
    pub exclude_links_from_lockfile: Option<bool>,
    /// `optimisticRepeatInstall` from `pnpm-workspace.yaml` /
    /// `~/.config/pnpm/config.yaml`. Defaults to `true` at the
    /// `Config` layer ([`Config::optimistic_repeat_install`]) to
    /// match pnpm.
    pub optimistic_repeat_install: Option<bool>,
    pub hoist_workspace_packages: Option<bool>,
    /// `extendNodePath` from `pnpm-workspace.yaml`. See
    /// [`Config::extend_node_path`].
    pub extend_node_path: Option<bool>,
    /// `preferSymlinkedExecutables` from `pnpm-workspace.yaml`. Unset by
    /// default: see [`Config::prefer_symlinked_executables`].
    pub prefer_symlinked_executables: Option<bool>,
    /// `linkWorkspacePackages` from `pnpm-workspace.yaml`. Tri-state
    /// (`true | false | "deep"`) — see [`LinkWorkspacePackages`].
    pub link_workspace_packages: Option<LinkWorkspacePackages>,
    /// `saveWorkspaceProtocol` from `pnpm-workspace.yaml`. Tri-state
    /// (`true | false | "rolling"`) — see [`SaveWorkspaceProtocol`].
    pub save_workspace_protocol: Option<SaveWorkspaceProtocol>,
    /// `injectWorkspacePackages` from `pnpm-workspace.yaml`. When
    /// `true`, every workspace-resolved dep is materialized as a
    /// `file:` (hard-linked copy) instead of a `link:` symlink. See
    /// [`Config::inject_workspace_packages`].
    pub inject_workspace_packages: Option<bool>,
    /// `hoistingLimits` from `pnpm-workspace.yaml`. One of `none`,
    /// `workspaces`, or `dependencies` — see
    /// [`crate::HoistingLimits`]. Missing → default
    /// [`crate::HoistingLimits::None`].
    pub hoisting_limits: Option<HoistingLimits>,
    /// `externalDependencies` from `pnpm-workspace.yaml`. Names
    /// whose top-level slot is reserved for an external linker
    /// and stripped from the hoist tree. Empty / missing → no
    /// externals.
    pub external_dependencies: Option<BTreeSet<String>>,
    pub dedupe_peer_dependents: Option<bool>,
    pub dedupe_peers: Option<bool>,
    pub dedupe_direct_deps: Option<bool>,
    pub prefer_workspace_packages: Option<bool>,
    pub dedupe_injected_deps: Option<bool>,
    pub strict_peer_dependencies: Option<bool>,
    pub ignore_compatibility_db: Option<bool>,
    pub resolve_peers_from_workspace_root: Option<bool>,
    pub block_exotic_subdeps: Option<bool>,
    pub verify_store_integrity: Option<bool>,
    pub strict_store_pkg_content_check: Option<bool>,
    pub include_workspace_root: Option<bool>,
    pub ignore_workspace_cycles: Option<bool>,
    pub disallow_workspace_cycles: Option<bool>,
    /// `frozenStore` from `pnpm-workspace.yaml`. Opens the store
    /// read-only and suppresses every store write — see
    /// [`Config::frozen_store`]. Default `false`.
    ///
    /// [`Config::frozen_store`]: crate::Config::frozen_store
    pub frozen_store: Option<bool>,
    /// `sideEffectsCache`: whether a build is restored, whether one is saved,
    /// and where from. A bare boolean sets reading and writing together.
    pub side_effects_cache: Option<SideEffectsCacheSetting>,
    /// The boolean spelling of `sideEffectsCache: { read: true, write: false }`.
    pub side_effects_cache_readonly: Option<bool>,
    pub fetch_retries: Option<u32>,
    pub fetch_retry_factor: Option<u32>,
    pub fetch_retry_mintimeout: Option<u64>,
    pub fetch_retry_maxtimeout: Option<u64>,
    pub network_concurrency: Option<usize>,
    /// `maxSockets` — per-origin concurrent-connection cap. See
    /// [`Config::max_sockets`]. Default unset (no per-origin cap).
    pub max_sockets: Option<usize>,
    /// `maxsockets` — npm's spelling of [`Self::max_sockets`], which pnpm
    /// reads too. A field of its own rather than a serde alias, because a
    /// file carrying both spellings is a duplicate field to serde and
    /// would fail the whole parse; pnpm takes it and lets the canonical
    /// spelling win.
    pub maxsockets: Option<usize>,
    pub fetch_timeout: Option<u64>,
    /// The `fetchWarnTimeoutMs` YAML value in milliseconds. [`None`] leaves
    /// [`Config::fetch_warn_timeout_ms`] unchanged.
    pub fetch_warn_timeout_ms: Option<u64>,
    /// The `fetchMinSpeedKiBps` YAML value in KiB/s. [`None`] leaves
    /// [`Config::fetch_min_speed_ki_bps`] unchanged.
    pub fetch_min_speed_ki_bps: Option<u64>,
    pub user_agent: Option<String>,
    /// `npmrcAuthFile` is read only from the global `config.yaml`
    /// (consumed by [`crate::Config::current`] to choose the user-level
    /// `.npmrc`); it is deliberately *not* in the `apply!` list, so a
    /// project `pnpm-workspace.yaml` declaring it is a no-op — matching
    /// pnpm, which sources the key from the global manifest only.
    pub npmrc_auth_file: Option<String>,

    /// Map of `name[@version]` → patch-file path (relative to the
    /// workspace dir or absolute). Read verbatim; relative-path
    /// resolution, file hashing, and grouping are deferred to
    /// [`pnpm_patching::resolve_and_group`] so the yaml layer
    /// stays pure data.
    ///
    /// [`IndexMap`] (not [`BTreeMap`]) — pnpm's JS-object iteration
    /// preserves the user's order, and that order leaks into
    /// `PATCH_KEY_CONFLICT` diagnostics that list matched ranges.
    /// Sorting the keys here would surface as a divergence in
    /// error messages.
    ///
    /// pnpm 10+ moved `patchedDependencies` out of
    /// `package.json#pnpm` into `pnpm-workspace.yaml`; pacquet
    /// matches that. The legacy `package.json#pnpm.patchedDependencies`
    /// shape is no longer consulted.
    ///
    /// [`BTreeMap`]: std::collections::BTreeMap
    pub patched_dependencies: Option<IndexMap<String, String>>,

    pub patches_dir: Option<String>,

    pub pnpmfile: Option<PnpmfileSetting>,

    /// `globalPnpmfile`. Unlike [`Self::pnpmfile`] this survives
    /// [`Self::clear_workspace_only_fields`]: pnpm lists `global-pnpmfile`
    /// among the keys its global `config.yaml` accepts.
    pub global_pnpmfile: Option<String>,

    /// `allowUnusedPatches` from `pnpm-workspace.yaml`. Default `false`.
    pub allow_unused_patches: Option<bool>,

    /// `configDependencies` from `pnpm-workspace.yaml`: package name →
    /// version-with-integrity spec. pnpm records this verbatim in the
    /// workspace-state file so that `checkDepsStatus` can detect when a
    /// config dependency changed and force a reinstall. Pacquet must
    /// write the same value back (see
    /// [`build_workspace_state`](../../package-manager/src/install.rs)),
    /// otherwise pnpm reads a missing `configDependencies` on the next
    /// `pnpm run` / `pnpm node`, compares it against the live config,
    /// and reinstalls on every invocation.
    pub config_dependencies: Option<BTreeMap<String, ConfigDependency>>,

    /// Map of `name[@version]` → [`AllowBuild`]. Drives pnpm 11's
    /// default-deny build policy: a package's lifecycle scripts only
    /// run when an entry here resolves to `true`.
    ///
    /// pnpm 10+ moved `allowBuilds` out of `package.json#pnpm` into
    /// `pnpm-workspace.yaml` alongside other install settings.
    pub allow_builds: Option<HashMap<String, AllowBuild>>,

    /// The workspace-structural keys of `pnpm-workspace.yaml`, carried so
    /// `pnpm config get` / `pnpm config list` can show them. Installs read
    /// them from the workspace-manifest layer, not from [`Config`], so
    /// [`Self::apply_to`] leaves them alone and the global `config.yaml`
    /// refuses them.
    pub packages: Option<Vec<String>>,
    /// See [`Self::packages`].
    pub catalog: Option<IndexMap<String, String>>,
    /// See [`Self::packages`].
    pub catalogs: Option<IndexMap<String, IndexMap<String, String>>>,
    /// See [`Self::packages`].
    pub only_built_dependencies: Option<Vec<String>>,
    /// See [`Self::packages`].
    pub never_built_dependencies: Option<Vec<String>>,
    /// See [`Self::packages`].
    pub ignored_built_dependencies: Option<Vec<String>>,

    /// Bypass the [`allow_builds`] gate entirely — every package may
    /// run lifecycle scripts. Same `pnpm-workspace.yaml` migration
    /// as `allowBuilds`. Default `false`.
    ///
    /// [`allow_builds`]: Self::allow_builds
    pub dangerously_allow_all_builds: Option<bool>,

    /// `strictDepBuilds` from `pnpm-workspace.yaml`. When `true` (the
    /// default), an install that ignored any dependency build script
    /// fails instead of only warning. Default `true`.
    pub strict_dep_builds: Option<bool>,

    /// `ignoreScripts` from `pnpm-workspace.yaml`. When `true`, no
    /// lifecycle scripts run and ignored dependency builds aren't
    /// collected. See [`Config::ignore_scripts`]. The `--ignore-scripts`
    /// CLI flag ORs on top of this. Default `false`.
    pub ignore_scripts: Option<bool>,

    /// `ignorePnpmfile` from `pnpm-workspace.yaml`. When `true`, no pnpmfile
    /// hooks run. See [`Config::ignore_pnpmfile`]. The `--ignore-pnpmfile` CLI
    /// flag ORs on top of this. Cleared by
    /// [`Self::clear_workspace_only_fields`], so the global `config.yaml`
    /// cannot set it. Default `false`.
    pub ignore_pnpmfile: Option<bool>,

    /// `gitChecks` from `pnpm-workspace.yaml`. When `false`, `pnpm publish`
    /// skips its git working-tree checks. See [`Config::git_checks`]. The
    /// `--no-git-checks` CLI flag forces it off on top of this. Default
    /// `true`.
    pub git_checks: Option<bool>,

    /// `engineStrict` from `pnpm-workspace.yaml` / global `config.yaml`.
    /// See [`Config::engine_strict`]. Default `false`.
    pub engine_strict: Option<bool>,

    /// `nodeVersion` from `pnpm-workspace.yaml` / global `config.yaml`.
    /// See [`Config::node_version`]. Default unset (auto-detect).
    pub node_version: Option<String>,

    /// `runtimeOnFail` from `pnpm-workspace.yaml` / global `config.yaml`.
    pub runtime_on_fail: Option<RuntimeOnFail>,

    /// Per-release-channel Node.js download mirrors.
    pub node_download_mirrors: Option<HashMap<String, String>>,

    /// `scriptsPrependNodePath` from `pnpm-workspace.yaml`. Tri-state
    /// — yaml accepts `true` / `false` / `"warn-only"`. Custom serde
    /// shape, see [`ScriptsPrependNodePath`]'s `Deserialize` impl.
    pub scripts_prepend_node_path: Option<ScriptsPrependNodePath>,

    /// `enablePrePostScripts` from `pnpm-workspace.yaml`. See
    /// [`Config::enable_pre_post_scripts`].
    pub enable_pre_post_scripts: Option<bool>,

    /// Tri-state `scriptShell` from `pnpm-workspace.yaml`. pnpm reads
    /// workspace settings into an object and assigns each present key
    /// onto the merged config, so an explicit `scriptShell: null`
    /// clears a value inherited from global `config.yaml`, while an
    /// absent key inherits. The extra `Option` layer preserves that
    /// distinction (same `deserialize_double_option` shape as
    /// `hoist_pattern`).
    ///
    /// See [`Config::script_shell`].
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub script_shell: Option<Option<String>>,

    /// Tri-state `nodeOptions` from `pnpm-workspace.yaml`. Same
    /// inherit / clear / set semantics as [`Self::script_shell`] — an
    /// explicit `nodeOptions: null` unsets an inherited `NODE_OPTIONS`.
    /// See [`Config::node_options`].
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub node_options: Option<Option<String>>,

    /// `unsafePerm` from `pnpm-workspace.yaml`. Forced to `true` on
    /// Windows in `apply_to`, matching pnpm.
    pub unsafe_perm: Option<bool>,

    /// `childConcurrency` from `pnpm-workspace.yaml`. Resolved
    /// through [`crate::resolve_child_concurrency`] in `apply_to`.
    /// Signed `i32` here so negative values (interpreted as
    /// `parallelism - |value|`) round-trip cleanly.
    pub child_concurrency: Option<i32>,

    /// `workspaceConcurrency` from `pnpm-workspace.yaml` / global
    /// `config.yaml`. Resolved through
    /// [`crate::resolve_child_concurrency`] in `apply_to`, the same
    /// way `childConcurrency` is. Signed `i32` so negative values
    /// (interpreted as `parallelism - |value|`) round-trip cleanly.
    /// A genuine config-file key (so it is kept, not cleared, in
    /// [`Self::clear_workspace_only_fields`]).
    pub workspace_concurrency: Option<i32>,

    /// `gitShallowHosts` from `pnpm-workspace.yaml`. Overrides
    /// [`Config::git_shallow_hosts`] wholesale when set —
    /// `pnpm-workspace.yaml` replaces the built-in defaults rather
    /// than merging.
    pub git_shallow_hosts: Option<Vec<String>>,

    /// `testPattern` from `pnpm-workspace.yaml` — see
    /// [`Config::test_pattern`].
    pub test_pattern: Option<Vec<String>>,

    /// `changedFilesIgnorePattern` from `pnpm-workspace.yaml` — see
    /// [`Config::changed_files_ignore_pattern`].
    pub changed_files_ignore_pattern: Option<Vec<String>>,

    /// `legacyDirFiltering` from `pnpm-workspace.yaml` — see
    /// [`Config::legacy_dir_filtering`].
    ///
    /// [`Config::legacy_dir_filtering`]: crate::Config::legacy_dir_filtering
    pub legacy_dir_filtering: Option<bool>,

    /// `syncInjectedDepsAfterScripts` from `pnpm-workspace.yaml` — see
    /// [`Config::sync_injected_deps_after_scripts`].
    pub sync_injected_deps_after_scripts: Option<Vec<String>>,

    /// `supportedArchitectures` from `pnpm-workspace.yaml`. Drives the
    /// optional-dependency platform check at install time: a
    /// `name: ['darwin'], cpu: ['arm64']` setting tells pacquet to
    /// keep `darwin-arm64` variants of platform-tagged packages even
    /// on a non-matching host. Per-axis CLI flags (`--cpu`, `--libc`,
    /// `--os`) override individual axes.
    /// Read from yaml verbatim (no `current` substitution here — that
    /// happens at the [`pnpm_package_is_installable::check_platform`]
    /// call site where the host triple is in scope).
    pub supported_architectures: Option<SupportedArchitectures>,

    /// `ignoredOptionalDependencies` from `pnpm-workspace.yaml`: a
    /// list of dep-name patterns whose matching entries get
    /// stripped from every manifest's `optionalDependencies` (and
    /// `dependencies`, when a package lists the same name in both)
    /// before any consumer sees them. The setting also participates
    /// in the lockfile-side drift check.
    pub ignored_optional_dependencies: Option<Vec<String>>,

    /// `overrides` from `pnpm-workspace.yaml`: a `selector → spec`
    /// map that rewrites dependency specifiers everywhere they appear
    /// during install (both direct manifests and transitive
    /// packuments). Outer key encodes the override scope (bare name,
    /// `name@range`, or `parent>child` forms — see
    /// `pnpm_config_parse_overrides`); value is the replacement
    /// spec, or `-` to delete the dep entirely.
    ///
    /// Values are validated as strings at load time
    /// (`ERR_PNPM_INVALID_OVERRIDES`) and `$dep-name` self-references
    /// against the manifest's direct deps are resolved before
    /// downstream code sees them. Empty maps are normalized to
    /// `None` so the overrides key is dropped entirely.
    ///
    /// pnpm 10+ moved `overrides` out of `package.json#pnpm` into
    /// `pnpm-workspace.yaml`. Pacquet matches that — the legacy
    /// `package.json#pnpm.overrides` shape is no longer consulted.
    ///
    /// Lockfile drift: the raw map is recorded in `pnpm-lock.yaml`'s
    /// `overrides:` field. On a subsequent install,
    /// `pnpm_lockfile::check_lockfile_settings` compares this
    /// against `lockfile.overrides` and raises `OverridesChanged`
    /// on mismatch.
    pub overrides: Option<IndexMap<String, String>>,

    /// `cacheDir` from `pnpm-workspace.yaml`. Resolved against the
    /// workspace dir like the other path-valued fields. Drives
    /// the lockfile-verified JSONL cache + packument mirror used
    /// by the verifier.
    pub cache_dir: Option<String>,

    /// `dlxCacheMaxAge` from `pnpm-workspace.yaml`. Minutes; see
    /// [`Config::dlx_cache_max_age`].
    pub dlx_cache_max_age: Option<u64>,

    /// `minimumReleaseAge` from `pnpm-workspace.yaml`. Milliseconds;
    /// see [`Config::minimum_release_age`].
    pub minimum_release_age: Option<u64>,

    /// `minimumReleaseAgeExclude` from `pnpm-workspace.yaml`.
    pub minimum_release_age_exclude: Option<Vec<String>>,

    /// `minimumReleaseAgeExcludePrune` from `pnpm-workspace.yaml`.
    /// See [`Config::minimum_release_age_exclude_prune`]. Default
    /// `false`.
    pub minimum_release_age_exclude_prune: Option<bool>,

    /// `minimumReleaseAgeIgnoreMissingTime` from `pnpm-workspace.yaml`.
    pub minimum_release_age_ignore_missing_time: Option<bool>,

    /// `minimumReleaseAgeStrict` from `pnpm-workspace.yaml`.
    pub minimum_release_age_strict: Option<bool>,

    /// `trustLockfile` from `pnpm-workspace.yaml`. When `true`, the
    /// install skips the supply-chain verification pass entirely
    /// (see [`Config::trust_lockfile`]).
    ///
    /// [`Config::trust_lockfile`]: crate::Config::trust_lockfile
    pub trust_lockfile: Option<bool>,

    /// `trustPolicy` from `pnpm-workspace.yaml`. See [`TrustPolicy`].
    pub trust_policy: Option<TrustPolicy>,

    /// `initPackageManager` from `pnpm-workspace.yaml` /
    /// `~/.config/pnpm/config.yaml`. See
    /// [`Config::init_package_manager`].
    ///
    /// [`Config::init_package_manager`]: crate::Config::init_package_manager
    pub init_package_manager: Option<bool>,

    /// `initType` from `pnpm-workspace.yaml` /
    /// `~/.config/pnpm/config.yaml`. See [`InitType`].
    pub init_type: Option<InitType>,

    /// `initAuthorName` from `pnpm-workspace.yaml` /
    /// `~/.config/pnpm/config.yaml`. See [`Config::init_author_name`].
    ///
    /// [`Config::init_author_name`]: crate::Config::init_author_name
    pub init_author_name: Option<String>,

    /// `initAuthorEmail` from `pnpm-workspace.yaml` /
    /// `~/.config/pnpm/config.yaml`. See [`Config::init_author_email`].
    ///
    /// [`Config::init_author_email`]: crate::Config::init_author_email
    pub init_author_email: Option<String>,

    /// `initAuthorUrl` from `pnpm-workspace.yaml` /
    /// `~/.config/pnpm/config.yaml`. See [`Config::init_author_url`].
    ///
    /// [`Config::init_author_url`]: crate::Config::init_author_url
    pub init_author_url: Option<String>,

    /// `initLicense` from `pnpm-workspace.yaml` /
    /// `~/.config/pnpm/config.yaml`. See [`Config::init_license`].
    ///
    /// [`Config::init_license`]: crate::Config::init_license
    pub init_license: Option<String>,

    /// `initVersion` from `pnpm-workspace.yaml` /
    /// `~/.config/pnpm/config.yaml`. See [`Config::init_version`].
    ///
    /// [`Config::init_version`]: crate::Config::init_version
    pub init_version: Option<String>,

    /// `pmOnFail` from `pnpm-workspace.yaml`. See [`PmOnFail`].
    pub pm_on_fail: Option<PmOnFail>,

    /// `verifyDepsBeforeRun` from `pnpm-workspace.yaml` /
    /// `~/.config/pnpm/config.yaml`. See [`VerifyDepsBeforeRun`].
    pub verify_deps_before_run: Option<VerifyDepsBeforeRun>,

    /// `audit` from `pnpm-workspace.yaml`. Supersedes `auditLevel` and
    /// `auditConfig`; see [`AuditSettings`]. When both a value and its
    /// deprecated counterpart are set, `audit` wins (with a warning) —
    /// the mapping onto [`Config::audit_level`] / [`Config::audit_config`]
    /// happens in [`Self::apply_to`].
    pub audit: Option<AuditSettings>,

    /// `auditLevel` from `pnpm-workspace.yaml`.
    ///
    /// Deprecated in favor of [`AuditSettings::level`], kept for backward
    /// compatibility until the next major version.
    pub audit_level: Option<AuditLevel>,

    /// `auditConfig` from `pnpm-workspace.yaml`.
    ///
    /// Deprecated in favor of [`AuditSettings::ignore`], kept for backward
    /// compatibility until the next major version.
    pub audit_config: Option<AuditConfig>,

    /// `versioning` from `pnpm-workspace.yaml`: native workspace release
    /// management (fixed groups, ignore list, maxBump cap, per-package
    /// prerelease lines, changelog settings).
    pub versioning: Option<pnpm_versioning::VersioningSettings>,

    /// `trustPolicyExclude` from `pnpm-workspace.yaml`.
    pub trust_policy_exclude: Option<Vec<String>>,

    /// `trustPolicyIgnoreAfter` from `pnpm-workspace.yaml`. Minutes.
    pub trust_policy_ignore_after: Option<u64>,

    /// `packageExtensions` from `pnpm-workspace.yaml`: a
    /// `selector → extension` map that augments dependency manifests
    /// at install time. Outer key is a `name[@range]` selector; inner
    /// value lists the extra `dependencies`, `optionalDependencies`,
    /// `peerDependencies`, and `peerDependenciesMeta` entries to merge
    /// onto every matching manifest before the resolver walks it.
    ///
    /// `IndexMap` keeps insertion order so the hash-and-checksum side
    /// (a separate slice) can keep the same key ordering pnpm does.
    pub package_extensions: Option<IndexMap<String, PackageExtension>>,

    /// `resolutionMode` from `pnpm-workspace.yaml`. See
    /// [`ResolutionMode`].
    pub resolution_mode: Option<ResolutionMode>,

    /// `catalogMode` from `pnpm-workspace.yaml`. See [`CatalogMode`].
    pub catalog_mode: Option<CatalogMode>,

    /// `catalogPrune` from `pnpm-workspace.yaml`. See
    /// [`Config::catalog_prune`]. Default `false`.
    pub catalog_prune: Option<bool>,

    /// `catalogPrune`'s former name, still accepted. [`Self::catalog_prune`]
    /// wins when a file carries both.
    pub cleanup_unused_catalogs: Option<bool>,

    /// `saveCatalogName` from `pnpm-workspace.yaml`. See
    /// [`Config::save_catalog_name`].
    ///
    /// [`Config::save_catalog_name`]: crate::Config::save_catalog_name
    pub save_catalog_name: Option<String>,

    /// `savePrefix` from `pnpm-workspace.yaml`. See
    /// [`Config::save_prefix`].
    ///
    /// [`Config::save_prefix`]: crate::Config::save_prefix
    pub save_prefix: Option<String>,

    /// `saveExact` from `pnpm-workspace.yaml`. See
    /// [`Config::save_exact`]. Default `false`.
    ///
    /// [`Config::save_exact`]: crate::Config::save_exact
    pub save_exact: Option<bool>,

    /// `savePeer` from `pnpm-workspace.yaml`. See
    /// [`Config::save_peer`]. Default `false`.
    ///
    /// [`Config::save_peer`]: crate::Config::save_peer
    pub save_peer: Option<bool>,

    /// `registrySupportsTimeField` from `pnpm-workspace.yaml`. See
    /// [`Config::registry_supports_time_field`].
    ///
    /// [`Config::registry_supports_time_field`]: crate::Config::registry_supports_time_field
    pub registry_supports_time_field: Option<bool>,

    /// `allowedDeprecatedVersions` from `pnpm-workspace.yaml`. See
    /// [`Config::allowed_deprecated_versions`].
    ///
    /// [`Config::allowed_deprecated_versions`]: crate::Config::allowed_deprecated_versions
    pub allowed_deprecated_versions: Option<BTreeMap<String, String>>,

    /// `update` from `pnpm-workspace.yaml`. Supersedes `updateConfig`;
    /// see [`UpdateSettings`]. When both are set, `update` wins (with a
    /// warning) — the mapping onto [`Config::update_config`] happens in
    /// [`Self::apply_to`].
    pub update: Option<UpdateSettings>,

    /// `updateConfig` from `pnpm-workspace.yaml`. See [`UpdateConfig`].
    ///
    /// Deprecated in favor of [`Self::update`], kept for backward
    /// compatibility until the next major version.
    pub update_config: Option<UpdateConfig>,

    /// `peerDependencyRules` from `pnpm-workspace.yaml`. See
    /// [`PeerDependencyRules`].
    pub peer_dependency_rules: Option<PeerDependencyRules>,

    /// `tasks` from `pnpm-workspace.yaml`: the workspace's task
    /// declarations, keyed by task (script) name. See [`TaskSettings`].
    pub tasks: Option<IndexMap<String, TaskSettings>>,

    /// The problem keys [`Self::collect_key_issues`] found in the file this
    /// was parsed from. Not a setting: carried here so the CLI can report
    /// them at the point where it knows how severe they are (see the
    /// warnings/error in `pnpm-cli`'s `config_warnings`).
    #[serde(skip)]
    pub key_issues: WorkspaceKeyIssues,
}

/// The keys of a project's `pnpm-workspace.yaml` that set nothing, bucketed
/// by why: refused values a project may not contribute, keys naming no
/// setting any supported pnpm reads, and kebab-case spellings of keys pnpm
/// only reads in camelCase.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct WorkspaceKeyIssues {
    pub refused: Vec<String>,
    pub unrecognized: Vec<String>,
    pub non_camel_case: Vec<String>,
}

impl WorkspaceKeyIssues {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.refused.is_empty() && self.unrecognized.is_empty() && self.non_camel_case.is_empty()
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
    /// [`AuditConfig::ignore_ghsas`].
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
pub struct TaskSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<i64>,

    #[serde(skip)]
    invalid_concurrency: Option<serde_json::Value>,

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

    /// Fields this version of pnpm does not read, kept so validation can
    /// reject a typo instead of silently ignoring it.
    #[serde(flatten, skip_serializing_if = "IndexMap::is_empty")]
    pub unknown: IndexMap<String, serde_json::Value>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawTaskSettings {
    concurrency: Option<serde_json::Value>,
    depends_on: Option<Vec<String>>,
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
            depends_on: raw.depends_on,
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

/// Basename of the file pnpm reads; exported for test use.
pub const WORKSPACE_MANIFEST_FILENAME: &str = "pnpm-workspace.yaml";

/// Basename of pnpm's global config file inside `<configDir>`.
pub const GLOBAL_CONFIG_YAML_FILENAME: &str = "config.yaml";

/// Error when reading `pnpm-workspace.yaml`.
///
/// `ENOENT` is treated as "no manifest" and every other failure
/// propagates. `serde_saphyr::Error` is boxed so the returned
/// `Result` stays small.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum LoadWorkspaceYamlError {
    #[display("Failed to read pnpm-workspace.yaml at {}: {source}", path.display())]
    ReadFile {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },
    #[display("Failed to parse pnpm-workspace.yaml at {}: {source}", path.display())]
    ParseYaml {
        path: PathBuf,
        #[error(source)]
        source: Box<serde_saphyr::Error>,
    },
    /// The registry URL is redacted before it reaches this variant.
    #[display("The \"registries\" key {registry} embeds credentials")]
    #[diagnostic(
        code(ERR_PNPM_INVALID_SETTING),
        help("Put them in an .npmrc file instead, so they are not committed.")
    )]
    CredentialsInRegistryKey { registry: String },
    /// The registry URL is redacted before it reaches this variant.
    #[display(
        r#"The "registries[{registry:?}].{field}" setting is not allowed in pnpm-workspace.yaml"#
    )]
    #[diagnostic(
        code(ERR_PNPM_INVALID_SETTING),
        help("Set it in an .npmrc file instead, so it is not committed.")
    )]
    SecretInRegistryDeclaration { registry: String, field: String },
    /// The registry URL is redacted before it reaches this variant.
    #[display(r#"The "registries[{registry:?}].{field}" setting is not a known registry setting"#)]
    #[diagnostic(
        code(ERR_PNPM_INVALID_SETTING),
        help(r#"A registry declares "serverType", "scopes", and "prefix"."#)
    )]
    UnknownRegistryDeclarationField { registry: String, field: String },
    #[display(r#"The "registries" setting mixes registry declarations with "<scope>: <url>" entries ({scopes})"#)]
    #[diagnostic(
        code(ERR_PNPM_INVALID_SETTING),
        help(
            r#"Key every entry by registry URL and list the scopes routed to it under "scopes"."#
        )
    )]
    MixedRegistriesShapes { scopes: String },
    /// The registry URL is redacted before it reaches this variant.
    #[display(r#"The "registries[{registry:?}]" entry is a string"#)]
    #[diagnostic(
        code(ERR_PNPM_INVALID_SETTING),
        help(
            r#"A registry URL keys a declaration, e.g. {{ serverType: "artifactory" }}. A string value routes a scope, and a URL is not a scope."#
        )
    )]
    StringValuedRegistryDeclaration { registry: String },
    /// The registry URL is redacted before it reaches this variant.
    #[display(r#"The "registries[{registry:?}].scopes" setting should list "@"-prefixed scopes, but got {scope:?}"#)]
    #[diagnostic(
        code(ERR_PNPM_INVALID_SETTING),
        help(r#"A bare "@" is the scope-less default registry."#)
    )]
    RegistryScopeWithoutAtSign { registry: String, scope: String },
    /// The registry URLs are redacted before they reach this variant.
    #[display("The scope {scope:?} is routed to two registries: {registries}")]
    #[diagnostic(code(ERR_PNPM_INVALID_SETTING))]
    ScopeRoutedTwice { scope: String, registries: String },
    #[display("The prefix {prefix:?} is declared by two registries")]
    #[diagnostic(code(ERR_PNPM_INVALID_SETTING))]
    PrefixDeclaredTwice { prefix: String },
    #[display("The \"tasks['{task}'].{field}\" setting is not a known task setting")]
    #[diagnostic(
        code(ERR_PNPM_INVALID_SETTING),
        help(r#"A task declares "concurrency" and "dependsOn"."#)
    )]
    UnknownTaskSettingField { task: String, field: String },
    #[display(
        "The \"tasks['{task}'].concurrency\" setting should be a positive integer, but got {concurrency}"
    )]
    #[diagnostic(code(ERR_PNPM_INVALID_SETTING))]
    InvalidTaskConcurrency { task: String, concurrency: String },
    #[display(
        "The \"tasks['{task}'].dependsOn\" setting contains an entry with no task name: {entry:?}"
    )]
    #[diagnostic(code(ERR_PNPM_INVALID_SETTING))]
    EmptyTaskDependsOnEntry { task: String, entry: String },
    #[display("Invalid `_auth` setting: {source}")]
    InvalidJsonAuth {
        #[error(source)]
        source: serde_json::Error,
    },
    /// A `tokenHelper` was configured in a workspace or project `.npmrc`.
    /// It names an executable, so it is only honored from a trusted,
    /// non-repo source (`~/.npmrc` or the global `auth.ini`); a
    /// checked-in `.npmrc` must not be able to run an arbitrary command.
    #[display("tokenHelper must not be configured in project-level .npmrc")]
    #[diagnostic(
        code(ERR_PNPM_TOKEN_HELPER_IN_PROJECT_CONFIG),
        help(
            "The key {key:?} was found in project config. Move it to ~/.npmrc or the global pnpm auth.ini."
        )
    )]
    TokenHelperInProjectConfig { key: String },
    /// An `_auth` credential did not decode as base64. Its whole point is
    /// to carry `<username>:<password>` base64-encoded, so a value that
    /// cannot be decoded would otherwise reach the registry as a header
    /// no server can read — a silent 401 instead of a fixable error.
    #[display("Failed to decode {key} as base64")]
    #[diagnostic(
        code(ERR_PNPM_AUTH_INVALID_BASE64),
        help("{key} must hold the base64 encoding of <username>:<password>.")
    )]
    AuthInvalidBase64 { key: &'static str },
    /// A decoded `_auth` credential held no `:`, so it names no password.
    #[display("No separator found in the decoded form of _auth")]
    #[diagnostic(
        code(ERR_PNPM_AUTH_MISSING_SEPARATOR),
        help(
            "_auth is a base64 encoded form of <username>:<password> where the colon (:) serves as the separator"
        )
    )]
    AuthMissingSeparator,
    /// A honored `tokenHelper` value contained a character pnpm reserves
    /// for future quoting / interpolation support.
    #[display("Unexpected character {character:?} in tokenHelper")]
    #[diagnostic(
        code(ERR_PNPM_TOKEN_HELPER_UNSUPPORTED_CHARACTER),
        help(
            "Try wrapping the current command in a script whose name does not contain unsupported characters."
        )
    )]
    TokenHelperUnsupportedCharacter { character: char },
    /// The root manifest a `$dep-name` self-reference in `overrides`
    /// resolves against exists but could not be read or parsed.
    /// Boxed so the returned `Result` stays small.
    #[display("Failed to read the root package.json: {source}")]
    ReadRootManifest {
        #[error(source)]
        source: Box<pnpm_package_manifest::PackageManifestError>,
    },
    /// An `overrides` value used the `$dep-name` self-reference syntax,
    /// but the root manifest declares no such direct dependency.
    #[display(
        r#"Cannot resolve version {spec} in overrides. The direct dependencies don't have dependency "{dependency_name}"."#
    )]
    #[diagnostic(code(ERR_PNPM_CANNOT_RESOLVE_OVERRIDE_VERSION))]
    CannotResolveOverrideVersion { spec: String, dependency_name: String },

    /// The signing trust root for remote side-effects artifacts appeared in a
    /// committed file. Only the global config yaml and the environment may
    /// carry it — see [`RemoteSideEffectsCacheSettings`].
    #[display("{prefix}.{field} cannot be set by a workspace ({})", path.display())]
    #[diagnostic(
        code(ERR_PNPM_WORKSPACE_REMOTE_SIDE_EFFECTS_TRUST),
        help(
            "Set it in the global config file or in the environment instead of {}.",
            path.display(),
        )
    )]
    WorkspaceRemoteSideEffectsTrust { path: PathBuf, prefix: &'static str, field: &'static str },
}

impl WorkspaceSettings {
    /// Read the global config.yaml at `<config_dir>/config.yaml`, if
    /// present.
    ///
    /// This file uses the same parser as `pnpm-workspace.yaml`, but a
    /// key-filter pass ([`Self::clear_workspace_only_fields`]) drops
    /// workspace-only knobs (`nodeLinker`, `hoist`, `lockfile`, ...)
    /// so they cannot be set globally.
    ///
    /// Returns `Ok(None)` when the file does not exist. Read or parse
    /// failures propagate.
    pub fn load_global(config_dir: &Path) -> Result<Option<Self>, LoadWorkspaceYamlError> {
        let path = config_dir.join(GLOBAL_CONFIG_YAML_FILENAME);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(LoadWorkspaceYamlError::ReadFile { path, source }),
        };
        let mut settings: WorkspaceSettings = serde_saphyr::from_str(&text)
            .map_err(Box::new)
            .map_err(|source| LoadWorkspaceYamlError::ParseYaml { path: path.clone(), source })?;
        settings.validate_registries()?;
        settings.validate_tasks()?;
        settings.clear_workspace_only_fields();
        settings.warn_about_dropped_keys(&text, &path);
        Ok(Some(settings))
    }

    /// Reject a `registries` map pnpm would read as something other than what
    /// it says. See [`registries::validate`] for the rules.
    ///
    /// Checked after parsing rather than in a `Deserialize` impl on purpose:
    /// `serde_saphyr` renders the offending source line verbatim under its
    /// errors, so rejecting at parse time would print the very credential
    /// being rejected into the terminal and any CI log.
    fn validate_registries(&self) -> Result<(), LoadWorkspaceYamlError> {
        let Some(entries) = self.registries.as_ref() else { return Ok(()) };
        registries::validate(entries)
    }

    /// The `tasks` section feeds the task-graph builder of `pnpm -r run`,
    /// which reads it without further checks — a malformed entry has to be
    /// rejected here rather than surface as a scheduling bug far from the
    /// setting that produced it.
    fn validate_tasks(&self) -> Result<(), LoadWorkspaceYamlError> {
        let Some(tasks) = self.tasks.as_ref() else { return Ok(()) };
        for (task, settings) in tasks {
            if let Some(field) = settings.unknown.keys().next() {
                return Err(LoadWorkspaceYamlError::UnknownTaskSettingField {
                    task: task.clone(),
                    field: field.clone(),
                });
            }
            if let Some(concurrency) = settings.concurrency
                && concurrency < 1
            {
                return Err(LoadWorkspaceYamlError::InvalidTaskConcurrency {
                    task: task.clone(),
                    concurrency: concurrency.to_string(),
                });
            }
            if let Some(concurrency) = settings.invalid_concurrency.as_ref() {
                return Err(LoadWorkspaceYamlError::InvalidTaskConcurrency {
                    task: task.clone(),
                    concurrency: concurrency.to_string(),
                });
            }
            for entry in settings.depends_on.iter().flatten() {
                if entry.is_empty() || entry == "^" {
                    return Err(LoadWorkspaceYamlError::EmptyTaskDependsOnEntry {
                        task: task.clone(),
                        entry: entry.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    /// Warn about the keys of the global `config.yaml` that never reach the
    /// settings, in the three messages pnpm emits for that file.
    ///
    /// What survived is read back off `self` rather than off a second list of
    /// key names, which would drift from the struct: a key serde did not
    /// recognize is absent from the serialized settings, and one
    /// [`Self::clear_workspace_only_fields`] zeroed is null there.
    ///
    /// A dropped camelCase key pnpm's `isConfigFileKey` accepts stays silent:
    /// pnpm honors it in this file, so the fix is to honor it too, and until
    /// then a warning would diverge from pnpm's output on the same file.
    fn warn_about_dropped_keys(&self, text: &str, path: &Path) {
        let Ok(document) = serde_saphyr::from_str::<IndexMap<String, Option<IgnoredAny>>>(text)
        else {
            return;
        };
        let Ok(serde_json::Value::Object(kept)) = serde_json::to_value(self) else {
            return;
        };

        let mut movable = Vec::new();
        let mut unrecognized = Vec::new();
        let mut nowhere = Vec::new();
        let mut kebab_case = Vec::new();
        for key in document.iter().filter(|(_, value)| value.is_some()).map(|(key, _)| key) {
            if key == SCHEMA_DIRECTIVE_KEY {
                continue;
            }
            if matches!(kept.get(key), Some(value) if !value.is_null()) {
                continue;
            }
            // The key comes from a file the machine's user controls, but the
            // same rendering serves the project file, so it is sanitized here
            // too rather than only where it must be.
            let key = redact_and_sanitize(key);
            let key = key.as_str();
            if !is_config_file_key(&to_kebab_case(key)) {
                if is_refused_by_a_project_manifest(key) {
                    nowhere.push(format!(
                        r#""{key}" ({})"#,
                        where_refused_key_belongs(&to_camel_case(key)),
                    ));
                } else if is_known_setting_key(key) {
                    movable.push(format!(r#""{key}""#));
                } else {
                    unrecognized.push(annotate_unknown_setting(key));
                }
            } else if !is_camel_case(key) {
                kebab_case.push(format!(r#""{key}" (use "{}")"#, to_camel_case(key)));
            }
        }

        let path = path.display();
        if !movable.is_empty() {
            let movable = movable.join(", ");
            tracing::warn!(
                target: "pacquet::config",
                r#"The following settings cannot be set in the global config file ("{path}") and were ignored: {movable}. Move them to a project-level pnpm-workspace.yaml. To share these settings across projects, use config dependencies: https://pnpm.io/11.x/config-dependencies"#,
            );
        }
        if !unrecognized.is_empty() {
            let unrecognized = unrecognized.join(", ");
            tracing::warn!(
                target: "pacquet::config",
                r#"The following settings in the global config file ("{path}") are not recognized by this version of pnpm and were ignored: {unrecognized}."#,
            );
        }
        if !nowhere.is_empty() {
            let nowhere = nowhere.join(", ");
            tracing::warn!(
                target: "pacquet::config",
                r#"The following settings cannot be set in the global config file ("{path}") and were ignored: {nowhere}."#,
            );
        }
        if !kebab_case.is_empty() {
            let kebab_case = kebab_case.join(", ");
            tracing::warn!(
                target: "pacquet::config",
                r#"The following settings in the global config file ("{path}") were ignored because they are not written in camelCase: {kebab_case}."#,
            );
        }
    }

    /// Zero out the release-age and trust policies for `self-update`.
    ///
    /// `self-update` replaces the pnpm binary every later install runs
    /// through, so a repository must not get a say in whether it may be
    /// replaced. Both policies are dangerous in both directions here: a
    /// cooldown lowered waives the protection the user configured, raised it
    /// pins the machine to the installed pnpm — including past a release that
    /// fixes a vulnerability in it; a trust policy turned off accepts a pnpm
    /// release whose trust evidence the user meant to reject, turned on blocks
    /// the update the same way. Unlike a blocked dependency upgrade, those
    /// decisions follow the user out of the repository. The policies therefore
    /// come from the built-in defaults, the global `config.yaml`, and
    /// `PNPM_CONFIG_*` env vars only (plus CLI flags, applied by the caller).
    pub fn clear_self_update_policy(&mut self) {
        self.minimum_release_age = None;
        self.minimum_release_age_exclude = None;
        self.minimum_release_age_ignore_missing_time = None;
        self.minimum_release_age_strict = None;
        self.trust_policy = None;
        self.trust_policy_exclude = None;
        self.trust_policy_ignore_after = None;
    }

    /// Zero out fields not permitted in the global `config.yaml`.
    ///
    /// Every field listed here is a key excluded from the global
    /// config, plus the programmatic-only and workspace-only knobs
    /// (`patchedDependencies`, `allowBuilds`,
    /// `supportedArchitectures`, `ignoredOptionalDependencies`,
    /// `hoistingLimits`, `externalDependencies`) that pnpm only reads
    /// from `pnpm-workspace.yaml` or the legacy `package.json#pnpm`
    /// field. Without this filter a user could put `nodeLinker:
    /// hoisted` in `~/.config/pnpm/config.yaml` and pacquet would
    /// honor it while pnpm wouldn't — anti-parity.
    pub fn clear_workspace_only_fields(&mut self) {
        // Only the layout half of a registry declaration is workspace-only: it
        // decides which tarball URLs are omitted from the lockfile, so a
        // machine-local setting would make one developer write a lockfile
        // their collaborators read back with a different layout. The routes to
        // the registry are a legitimate global preference.
        for entry in self.registries.iter_mut().flat_map(BTreeMap::values_mut) {
            if let RegistryEntry::Declaration(declaration) = entry {
                declaration.server_type = None;
            }
        }
        self.versioning = None;
        self.packages = None;
        self.catalog = None;
        // Task declarations describe the workspace's own scripts; pnpm's
        // config-file key filter drops them from the global file too.
        self.tasks = None;
        // A pnpmfile belongs to the project that ships it, and pnpm reads
        // `ignorePnpmfile` from `pnpm-workspace.yaml` and the environment but
        // not from here. Honoring it globally would silently drop a
        // repository's hooks on one machine and resolve a different graph.
        self.ignore_pnpmfile = None;
        self.catalogs = None;
        self.only_built_dependencies = None;
        self.never_built_dependencies = None;
        self.ignored_built_dependencies = None;
        self.hoist = None;
        self.embed_readme = None;
        self.ignore_workspace_root_check = None;
        self.pending = None;
        self.recursive_install = None;
        self.reverse = None;
        self.skip_manifest_obfuscation = None;
        self.sort = None;
        self.hoist_pattern = None;
        self.public_hoist_pattern = None;
        self.shamefully_hoist = None;
        self.modules_dir = None;
        self.node_linker = None;
        self.symlink = None;
        self.lockfile = None;
        self.frozen_lockfile = None;
        self.deploy_all_files = None;
        self.force_legacy_deploy = None;
        self.shared_workspace_lockfile = None;
        self.git_branch_lockfile = None;
        self.merge_git_branch_lockfiles = None;
        self.merge_git_branch_lockfiles_branch_pattern = None;
        self.offline = None;
        self.lockfile_include_tarball_url = None;
        self.auto_install_peers = None;
        self.auto_install_peers_from_highest_match = None;
        self.exclude_links_from_lockfile = None;
        self.hoist_workspace_packages = None;
        self.link_workspace_packages = None;
        self.save_workspace_protocol = None;
        self.inject_workspace_packages = None;
        self.dedupe_peer_dependents = None;
        self.dedupe_peers = None;
        self.dedupe_direct_deps = None;
        self.prefer_workspace_packages = None;
        self.dedupe_injected_deps = None;
        self.strict_peer_dependencies = None;
        self.ignore_compatibility_db = None;
        self.resolve_peers_from_workspace_root = None;
        self.block_exotic_subdeps = None;
        self.hoisting_limits = None;
        self.external_dependencies = None;
        self.patched_dependencies = None;
        self.pnpmfile = None;
        self.config_dependencies = None;
        self.allow_builds = None;
        self.supported_architectures = None;
        self.ignored_optional_dependencies = None;
        self.overrides = None;
        self.package_extensions = None;
        self.test_pattern = None;
        self.changed_files_ignore_pattern = None;
        self.legacy_dir_filtering = None;
        self.sync_injected_deps_after_scripts = None;
        self.allow_unused_patches = None;
        self.save_catalog_name = None;
        self.save_peer = None;
    }

    /// Read `<dir>/pnpm-workspace.yaml` without walking ancestors.
    /// Returns `Ok(None)` only when nothing exists at that exact path;
    /// every other error (including `EISDIR` for a directory named
    /// `pnpm-workspace.yaml`, or permission denied) propagates, matching
    /// pnpm where `ENOENT` is the only silent case.
    pub fn load_at(dir: &Path) -> Result<Option<Self>, LoadWorkspaceYamlError> {
        let path = dir.join(WORKSPACE_MANIFEST_FILENAME);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(LoadWorkspaceYamlError::ReadFile { path, source }),
        };
        let mut settings: WorkspaceSettings = text
            .pipe_as_ref(serde_saphyr::from_str)
            .map_err(Box::new)
            .map_err(|source| LoadWorkspaceYamlError::ParseYaml { path: path.clone(), source })?;
        settings.validate_registries()?;
        settings.validate_tasks()?;
        settings.reject_repo_controlled_trust_material(&path)?;
        settings.collect_key_issues(&text);
        Ok(Some(settings))
    }

    /// Reject every remote side-effects field a committed file may not set.
    ///
    /// A workspace declares which organization and packages are eligible and
    /// nothing else: the rest describes the act of signing — which key signs,
    /// what provenance the signature attests, and whether to publish at all —
    /// so it belongs to the machine holding the key. Letting a repository set
    /// `publish` would turn a key the machine holds for its own builds into a
    /// signing oracle any clone could aim at a registry of its choosing.
    ///
    /// Checked after parsing rather than through `deny_unknown_fields` because
    /// the same struct also parses the global config yaml, where every field is
    /// legitimate.
    fn reject_repo_controlled_trust_material(
        &self,
        path: &Path,
    ) -> Result<(), LoadWorkspaceYamlError> {
        let canonical = match self.side_effects_cache.as_ref() {
            Some(SideEffectsCacheSetting::Settings(settings)) => settings.remote.as_ref(),
            _ => None,
        };
        // The message names the spelling the file actually used, since telling
        // someone to move `remoteSideEffectsCache.privateKey` out of a file
        // that says `sideEffectsCache.remote.privateKey` sends them looking
        // for a key that is not there.
        for (setting, prefix) in [
            (canonical, "sideEffectsCache.remote"),
            (self.remote_side_effects_cache.as_ref(), "remoteSideEffectsCache"),
        ] {
            let Some(settings) = setting else { continue };
            Self::reject_machine_only_fields(settings, prefix, path)?;
        }
        Ok(())
    }

    fn reject_machine_only_fields(
        settings: &RemoteSideEffectsCacheSettings,
        prefix: &'static str,
        path: &Path,
    ) -> Result<(), LoadWorkspaceYamlError> {
        let machine_only = [
            ("publish", settings.publish.is_some()),
            ("keyId", settings.key_id.is_some()),
            ("builderId", settings.builder_id.is_some()),
            ("imageDigest", settings.image_digest.is_some()),
            ("architectureBaseline", settings.architecture_baseline.is_some()),
            ("buildEnv", settings.build_env.is_some()),
            ("trustedKeys", settings.trusted_keys.is_some()),
            ("privateKey", settings.private_key.is_some()),
        ];
        let Some((field, _)) = machine_only.into_iter().find(|(_, is_set)| *is_set) else {
            return Ok(());
        };
        Err(LoadWorkspaceYamlError::WorkspaceRemoteSideEffectsTrust {
            path: path.to_path_buf(),
            prefix,
            field,
        })
    }

    /// Bucket the file's keys that set nothing into [`Self::key_issues`],
    /// under the project-file rules: refused values, keys naming no setting
    /// any supported pnpm reads, and kebab-case spellings of known settings.
    /// Reporting is the caller's job — how severe an unrecognized key is
    /// depends on whether the running pnpm is the project's pinned version,
    /// which only the CLI layer knows.
    pub fn collect_key_issues(&mut self, text: &str) {
        if !Self::may_have_key_issues(text) {
            return;
        }
        let Ok(document) = serde_saphyr::from_str::<IndexMap<String, Option<IgnoredAny>>>(text)
        else {
            return;
        };
        let mut issues = WorkspaceKeyIssues::default();
        for key in document.iter().filter(|(_, value)| value.is_some()).map(|(key, _)| key) {
            if key == SCHEMA_DIRECTIVE_KEY {
                continue;
            }
            if is_refused_by_a_project_manifest(key) {
                issues.refused.push(key.clone());
            } else if !is_known_setting_key(key) {
                issues.unrecognized.push(key.clone());
            } else if !is_camel_case(key) {
                issues.non_camel_case.push(key.clone());
            }
        }
        self.key_issues = issues;
    }

    /// Whether `text` may carry a key [`WorkspaceSettings::collect_key_issues`]
    /// would report, answered without parsing the file a second time.
    ///
    /// Serde keeps no record of the keys it dropped, so collecting them means
    /// re-reading the document — which costs as much as the parse that
    /// produced the settings, on every command, to find nothing in the
    /// overwhelmingly common case of a file that is simply correct.
    ///
    /// The top-level keys are the least indented lines of the document, since
    /// everything a key nests under it is indented further; the root mapping
    /// may itself be indented, so what marks a top-level key is the smallest
    /// indentation the file uses rather than column zero. A line this cannot
    /// measure or classify counts as one to look at, so the answer errs only
    /// towards re-reading, never towards missing a key.
    fn may_have_key_issues(text: &str) -> bool {
        let content_lines = text.lines().filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.is_empty() && !trimmed.starts_with('#')
        });
        let mut root_indent = usize::MAX;
        for line in content_lines.clone() {
            let indent = line.len() - line.trim_start().len();
            // YAML forbids a tab as indentation, so a file that uses one is
            // not worth measuring against.
            if line[..indent].bytes().any(|byte| byte != b' ') {
                return true;
            }
            root_indent = root_indent.min(indent);
        }
        if root_indent == usize::MAX {
            return false;
        }
        content_lines.filter(|line| line.len() - line.trim_start().len() == root_indent).any(
            |line| {
                let Some((key, _)) = line.trim_start().split_once(':') else { return true };
                let key = key.trim_end();
                key != SCHEMA_DIRECTIVE_KEY
                    && (!is_camel_case(key)
                        || !is_known_setting_key(key)
                        || is_refused_by_a_project_manifest(key))
            },
        )
    }

    /// Walk up from `start_dir` looking for a readable `pnpm-workspace.yaml`.
    /// Returns `Ok(None)` if no ancestor has one. Per-level semantics are
    /// [`Self::load_at`]'s.
    pub fn find_and_load(
        start_dir: &Path,
    ) -> Result<Option<(PathBuf, Self)>, LoadWorkspaceYamlError> {
        for dir in start_dir.ancestors() {
            if let Some(settings) = Self::load_at(dir)? {
                return Ok(Some((dir.join(WORKSPACE_MANIFEST_FILENAME), settings)));
            }
        }
        Ok(None)
    }

    /// Expand `${VAR}` in trusted user-controlled settings.
    ///
    /// Call this before [`Self::apply_to`] so expanded values land in
    /// [`Config`].
    pub fn substitute_env_trusted<Sys: EnvVar>(&mut self) {
        self.substitute_env_scalars::<Sys>();
        substitute_optional_string::<Sys>(&mut self.pnpr_server);
        substitute_optional_string::<Sys>(&mut self.registry);
        substitute_optional_string::<Sys>(&mut self.https_proxy);
        substitute_optional_string::<Sys>(&mut self.http_proxy);
        substitute_optional_string::<Sys>(&mut self.proxy);
        substitute_json_string::<Sys>(&mut self.no_proxy);
        substitute_json_string::<Sys>(&mut self.noproxy);
        substitute_registry_entries::<Sys>(&mut self.registries);
        substitute_optional_string_map::<Sys>(&mut self.named_registries);
    }

    /// Expand `${VAR}` in ordinary string settings, but drop
    /// placeholders inside workspace-controlled request-destination
    /// fields. Scalar strings still have `${VAR}` expanded, while
    /// `registry`, `registries`, `namedRegistries`, and `pnprServer`
    /// are filtered instead of expanding environment variables into
    /// request URLs.
    ///
    /// Call this before [`Self::apply_to`] so expanded values land in
    /// [`Config`] and filtered values do not.
    pub fn substitute_env_untrusted<Sys: EnvVar>(&mut self) {
        self.substitute_env_scalars::<Sys>();

        if self.registry.as_deref().is_some_and(has_env_placeholder) {
            self.registry = None;
        }
        if let Some(registries) = self.registries.as_mut() {
            registries::retain_without_env_placeholders(registries, has_env_placeholder);
        }
        if let Some(named_registries) = self.named_registries.as_mut() {
            named_registries.retain(|_, value| !has_env_placeholder(value));
        }

        if self.pnpr_server.as_deref().is_some_and(has_env_placeholder) {
            self.pnpr_server = None;
        }
        for proxy in [&mut self.https_proxy, &mut self.http_proxy, &mut self.proxy] {
            if proxy.as_deref().is_some_and(has_env_placeholder) {
                *proxy = None;
            }
        }
        for no_proxy in [&mut self.no_proxy, &mut self.noproxy] {
            if no_proxy
                .as_ref()
                .and_then(serde_json::Value::as_str)
                .is_some_and(has_env_placeholder)
            {
                *no_proxy = None;
            }
        }
    }

    /// Rewrite a leading `~/` in `globalDir` / `globalBinDir` into the home
    /// directory, as pnpm's `transformGlobalDirKeys` does. A shell expands
    /// the tilde before `pnpm config set` sees it, but a hand-written
    /// `config.yaml` carries it verbatim.
    ///
    /// Call this before [`Self::apply_to`], which would otherwise take the
    /// tilde for an ordinary relative path segment.
    pub(crate) fn expand_global_dir_home_prefixes<Sys: GetHomeDir>(&mut self) {
        for dir in [&mut self.global_dir, &mut self.global_bin_dir] {
            let Some(relative) = dir
                .as_deref()
                .and_then(|dir| dir.strip_prefix("~/").or_else(|| dir.strip_prefix(r"~\")))
            else {
                continue;
            };
            if let Some(expanded) = Sys::home_dir()
                .map(|home_dir| join_home_relative(&home_dir, relative))
                .and_then(|expanded| expanded.into_os_string().into_string().ok())
            {
                *dir = Some(expanded);
            }
        }
    }

    fn substitute_env_scalars<Sys: EnvVar>(&mut self) {
        substitute_optional_string::<Sys>(&mut self.scope);
        substitute_optional_string::<Sys>(&mut self.store_dir);
        substitute_optional_string::<Sys>(&mut self.state_dir);
        substitute_optional_string::<Sys>(&mut self.modules_dir);
        substitute_optional_string::<Sys>(&mut self.virtual_store_dir);
        substitute_optional_string::<Sys>(&mut self.global_virtual_store_dir);
        substitute_optional_string::<Sys>(&mut self.global_dir);
        substitute_optional_string::<Sys>(&mut self.global_bin_dir);
        substitute_optional_string::<Sys>(&mut self.user_agent);
        substitute_optional_string::<Sys>(&mut self.npmrc_auth_file);
        substitute_optional_string::<Sys>(&mut self.lockfile_dir);
        substitute_optional_string::<Sys>(&mut self.patches_dir);
        substitute_optional_string::<Sys>(&mut self.cache_dir);
        substitute_optional_inner_string::<Sys>(&mut self.script_shell);
        substitute_optional_inner_string::<Sys>(&mut self.node_options);
    }

    /// Apply every set field onto `config`, leaving unset ones untouched.
    ///
    /// Path-valued settings are resolved against `base_dir` if relative —
    /// anchored at the workspace root where the yaml was found, matching pnpm.
    pub fn apply_to(self, config: &mut Config, base_dir: &Path) {
        self.apply_proxy_to(&mut config.proxy, &mut config.proxy_keys);

        // Captured before the `apply!` macro and audit if-lets below move
        // these out of `self`; consumed after, to warn on the redundant
        // combination of a new section key and its deprecated counterpart.
        let update_config_in_yaml = self.update_config.is_some();
        let audit_level_in_yaml = self.audit_level.is_some();
        let audit_config_in_yaml = self.audit_config.is_some();

        // `catalogPrune`'s former name, applied before the macro so the
        // canonical key wins when a file carries both.
        if let Some(v) = self.cleanup_unused_catalogs {
            config.catalog_prune = v;
        }

        // Tri-state on `Config`: `exec` treats "never asked" differently
        // from an explicit `false`, so the macro's "apply when set" shape
        // would collapse the distinction.
        if let Some(v) = self.reporter_hide_prefix {
            config.reporter_hide_prefix = Some(v);
        }

        // pnpm spells the setting `gitBranchLockfile` and exposes the
        // resolved answer as `useGitBranchLockfile`; the macro below can
        // only apply fields the two structs name identically.
        if let Some(v) = self.git_branch_lockfile {
            config.use_git_branch_lockfile = v;
        }

        // `virtualStoreType` is the canonical spelling of the boolean
        // `enableGlobalVirtualStore`, which the macro below applies. Both
        // land in the same field, so applying this after the macro is what
        // makes the canonical key win when a file carries both.
        let virtual_store_type = self.virtual_store_type;

        macro_rules! apply {
            ($($field:ident),* $(,)?) => {$(
                if let Some(v) = self.$field {
                    config.$field = v;
                }
            )*};
        }

        apply! {
            bail, ci, update_notifier, color, embed_readme, ignore_workspace_root_check,
            optional, package_lock, pending, recursive_install, reverse,
            stream, aggregate_output, use_stderr, ignore_workspace, shell_emulator,
            skip_manifest_obfuscation, sort, use_beta_cli,
            hoist, shamefully_hoist,
            node_linker, node_experimental_package_map, node_package_map_type,
            symlink, package_import_method, modules_cache_max_age,
            virtual_store_dir_max_length,
            peers_suffix_max_length,
            lockfile, prefer_frozen_lockfile,
            deploy_all_files, force_legacy_deploy, shared_workspace_lockfile,
            merge_git_branch_lockfiles, merge_git_branch_lockfiles_branch_pattern,
            offline, prefer_offline,
            lockfile_include_tarball_url,
            auto_install_peers, auto_install_peers_from_highest_match,
            exclude_links_from_lockfile,
            optimistic_repeat_install,
            init_package_manager,
            init_type,
            hoist_workspace_packages,
            extend_node_path,
            hoisting_limits, external_dependencies,
            dedupe_peer_dependents, dedupe_peers,
            dedupe_direct_deps, dedupe_injected_deps,
            strict_peer_dependencies, ignore_compatibility_db,
            resolve_peers_from_workspace_root, verify_store_integrity,
            strict_store_pkg_content_check, frozen_store,
            include_workspace_root,
            ignore_workspace_cycles, disallow_workspace_cycles,
            verify_deps_before_run,
            block_exotic_subdeps,
            link_workspace_packages,
            save_workspace_protocol,
            inject_workspace_packages,
            prefer_workspace_packages,
            side_effects_cache_readonly,
            fetch_retries, fetch_retry_factor,
            fetch_retry_mintimeout, fetch_retry_maxtimeout,
            network_concurrency, fetch_timeout,
            fetch_warn_timeout_ms, fetch_min_speed_ki_bps, user_agent,
            enable_global_virtual_store,
            virtual_store_only, enable_modules_dir,
            git_shallow_hosts,
            test_pattern, changed_files_ignore_pattern, legacy_dir_filtering,
            sync_injected_deps_after_scripts,
            resolution_mode, catalog_mode, catalog_prune,
            minimum_release_age_exclude_prune, save_peer, save_exact,
            registry_supports_time_field,
            allowed_deprecated_versions, update_config, peer_dependency_rules,
            enable_pre_post_scripts, dlx_cache_max_age,
            allow_unused_patches, tasks,
        }

        if let Some(virtual_store_type) = virtual_store_type {
            config.enable_global_virtual_store = virtual_store_type.is_global();
        }

        // `globalShims` merges key-wise instead of replacing,
        // so a layer can flip one package without restating the defaults.
        if let Some(global_shims) = self.global_shims {
            config.global_shims.apply(&global_shims);
        }

        // The `update` section supersedes the deprecated `updateConfig`.
        // Applied after the macro so it overrides an `updateConfig` set in
        // the same file; both together is redundant and warned about.
        if let Some(update) = self.update {
            if update_config_in_yaml {
                tracing::warn!(
                    target: "pacquet::config",
                    r#"Both the "update" and "updateConfig" settings are set. The deprecated "updateConfig" setting is ignored in favor of "update"."#,
                );
            }
            // The `update` section is authoritative when present, superseding
            // any deprecated `updateConfig`.
            config.update_config = UpdateConfig {
                ignore_dependencies: update.ignore_deps,
                changeset: update.changeset,
                github_actions: update.github_actions,
                github_actions_server: update.github_actions_server,
            };
        }

        if let Some(frozen_lockfile) = self.frozen_lockfile {
            config.frozen_lockfile = Some(frozen_lockfile);
        }
        if let Some(prefer_symlinked_executables) = self.prefer_symlinked_executables {
            config.prefer_symlinked_executables = Some(prefer_symlinked_executables);
        }
        if let Some(save_catalog_name) = self.save_catalog_name {
            config.save_catalog_name = Some(save_catalog_name);
        }
        if let Some(init_author_name) = self.init_author_name {
            config.init_author_name = Some(init_author_name);
        }
        if let Some(init_author_email) = self.init_author_email {
            config.init_author_email = Some(init_author_email);
        }
        if let Some(init_author_url) = self.init_author_url {
            config.init_author_url = Some(init_author_url);
        }
        if let Some(init_license) = self.init_license {
            config.init_license = Some(init_license);
        }
        if let Some(init_version) = self.init_version {
            config.init_version = Some(init_version);
        }
        if let Some(save_prefix) = self.save_prefix {
            config.save_prefix = Some(save_prefix);
        }

        if let Some(inner) = self.hoist_pattern {
            config.hoist_pattern = inner;
        }
        if let Some(inner) = self.public_hoist_pattern {
            config.public_hoist_pattern = inner;
        }

        // Applied AFTER `hoist_pattern` assignment so a yaml that sets
        // both `hoist: false` and `hoistPattern: ["..."]` still
        // disables — `hoist: false` wins.
        if !config.hoist {
            config.hoist_pattern = None;
        }

        if let Some(v) = self.modules_dir {
            config.modules_dir = resolve(base_dir, &v);
        }
        if let Some(v) = self.virtual_store_dir {
            config.virtual_store_dir = resolve(base_dir, &v);
        }
        if let Some(v) = self.global_virtual_store_dir {
            config.global_virtual_store_dir = resolve(base_dir, &v);
        }
        if let Some(v) = self.global_dir {
            config.global_dir = Some(resolve(base_dir, &v));
        }
        if let Some(v) = self.global_bin_dir {
            config.global_bin_dir = Some(resolve(base_dir, &v));
        }
        // Last of the path-valued settings: pinning the lockfile dir
        // re-resolves `modulesDir` / `virtualStoreDir` against it, so it
        // must see whatever this layer just set.
        if let Some(v) = self.lockfile_dir {
            config.pin_lockfile_dir(&resolve(base_dir, &v));
        }
        if let Some(v) = self.store_dir {
            config.store_dir = StoreDir::from(resolve(base_dir, &v));
        }
        let mut declared_prefixes = false;
        if let Some(entries) = self.registries {
            let lookups = registries::into_lookups(entries);
            declared_prefixes = !lookups.registries_by_prefix.is_empty();
            if let Some(registry) = lookups.default_registry {
                config.registry = registry;
            }
            config.registries_by_scope.extend(lookups.registries_by_scope);
            config.registries_by_prefix.extend(lookups.registries_by_prefix);
            config.registry_options_by_url.extend(lookups.registry_options_by_url);
        }
        if let Some(v) = self.registry {
            config.registry = normalize_registry_url(&v);
        }
        if let Some(v) = self.scope {
            config.scope = Some(v);
        }
        if let Some(v) = self.pnpr_server {
            config.pnpr_server = Some(v);
        }
        if let Some(v) = self.remote_side_effects_cache {
            config.remote_side_effects_cache.get_or_insert_default().overlay(v);
        }
        // The canonical declaration is applied after the alias so that it wins
        // where both are set, and its `remote` half overlays rather than
        // replaces: a repository names the organization while the machine
        // supplies the signing key, and neither may drop the other's fields.
        match self.side_effects_cache {
            Some(SideEffectsCacheSetting::Enabled(enabled)) => {
                config.apply_side_effects_cache_shorthand(enabled);
            }
            Some(SideEffectsCacheSetting::Settings(settings)) => {
                config.side_effects_cache_read_setting = Some(settings.read.unwrap_or(true));
                config.side_effects_cache_write_setting = Some(settings.write.unwrap_or(true));
                if let Some(remote) = settings.remote {
                    config.remote_side_effects_cache.get_or_insert_default().overlay(remote);
                }
            }
            None => {}
        }
        if let Some(v) = self.named_registries {
            if declared_prefixes {
                tracing::warn!(
                    target: "pacquet::config",
                    r#"Both the "registries" and "namedRegistries" settings declare registry prefixes. The deprecated "namedRegistries" setting is only read for prefixes "registries" does not declare."#,
                );
            }
            // A prefix a `registries` entry declares wins: this is the
            // deprecated spelling of the same thing.
            for (name, registry) in v {
                config.registries_by_prefix.entry(name).or_insert(registry);
            }
        }

        // Anchor patch-file path resolution against the workspace dir
        // (the yaml's parent), matching pnpm.
        config.workspace_dir = Some(base_dir.to_path_buf());
        if let Some(v) = self.patched_dependencies {
            config.patched_dependencies = Some(v);
        }
        if let Some(v) = self.patches_dir {
            config.patches_dir = Some(v);
        }
        if let Some(path) = self.global_pnpmfile {
            config.global_pnpmfile = Some(pnpm_fs::lexical_normalize(&base_dir.join(path)));
        }
        if let Some(pnpmfile) = self.pnpmfile {
            let paths = match pnpmfile {
                PnpmfileSetting::Single(path) => vec![path],
                PnpmfileSetting::Multiple(paths) => paths,
            };
            config.pnpmfile = Some(
                paths
                    .into_iter()
                    .map(|path| pnpm_fs::lexical_normalize(&base_dir.join(path)))
                    .collect(),
            );
        }
        if let Some(v) = self.config_dependencies {
            config.config_dependencies = Some(v);
        }
        if let Some(v) = self.allow_builds {
            config.allow_builds = decided_allow_builds(v);
        }
        if let Some(v) = self.dangerously_allow_all_builds {
            config.dangerously_allow_all_builds = v;
        }
        if let Some(v) = self.strict_dep_builds {
            config.strict_dep_builds = v;
        }
        if let Some(v) = self.ignore_scripts {
            config.ignore_scripts = v;
        }
        if let Some(v) = self.ignore_pnpmfile {
            config.ignore_pnpmfile = v;
        }
        if let Some(v) = self.git_checks {
            config.git_checks = v;
        }
        if let Some(v) = self.engine_strict {
            config.engine_strict = v;
        }
        if let Some(v) = self.node_version {
            config.node_version = Some(v);
        }
        if let Some(v) = self.runtime_on_fail {
            config.runtime_on_fail = Some(v);
        }
        if let Some(v) = self.node_download_mirrors {
            config.node_download_mirrors = v;
        }
        // npm's spelling first, so the canonical one wins when a single
        // file carries both.
        if let Some(v) = self.maxsockets {
            config.max_sockets = Some(v);
        }
        if let Some(v) = self.max_sockets {
            config.max_sockets = Some(v);
        }
        if let Some(v) = self.scripts_prepend_node_path {
            config.scripts_prepend_node_path = v;
        }
        if let Some(v) = self.script_shell {
            config.script_shell = v;
        }
        if let Some(v) = self.node_options {
            config.node_options = v;
        }
        if let Some(v) = self.unsafe_perm {
            config.unsafe_perm = v;
        }
        if cfg!(windows) {
            config.unsafe_perm = true;
        }
        if let Some(v) = self.child_concurrency {
            config.child_concurrency = resolve_child_concurrency(Some(v));
        }
        if let Some(v) = self.workspace_concurrency {
            config.workspace_concurrency = resolve_child_concurrency(Some(v));
        }
        if let Some(v) = self.supported_architectures {
            config.supported_architectures = Some(v);
        }
        if let Some(v) = self.ignored_optional_dependencies {
            config.ignored_optional_dependencies = Some(v);
        }
        // `$dep-name` self-references are resolved by
        // [`crate::override_version_references::resolve_version_references`]
        // once the cascade knows the workspace root, whose manifest
        // carries the direct dependencies they point at.
        if let Some(v) = self.overrides {
            config.overrides = (!v.is_empty()).then_some(v);
        }
        if let Some(v) = self.package_extensions {
            config.package_extensions = (!v.is_empty()).then_some(v);
        }
        if let Some(v) = self.cache_dir {
            config.cache_dir = resolve(base_dir, &v);
        }
        if let Some(v) = self.minimum_release_age {
            config.minimum_release_age = Some(v);
        }
        if let Some(v) = self.minimum_release_age_exclude {
            config.minimum_release_age_exclude = Some(v);
        }
        if let Some(v) = self.minimum_release_age_ignore_missing_time {
            config.minimum_release_age_ignore_missing_time = v;
        }
        if let Some(v) = self.minimum_release_age_strict {
            config.minimum_release_age_strict = Some(v);
        }
        if let Some(v) = self.trust_lockfile {
            config.trust_lockfile = v;
        }
        if let Some(v) = self.trust_policy {
            config.trust_policy = v;
        }
        if let Some(v) = self.pm_on_fail {
            config.pm_on_fail = Some(v);
        }
        if let Some(v) = self.audit_level {
            config.audit_level = Some(v);
        }
        if let Some(v) = self.audit_config {
            config.audit_config = v;
        }

        // The `audit` section supersedes the deprecated `auditLevel` and
        // `auditConfig`. Applied after them so it overrides values set in the
        // same file; each redundant pairing is warned about.
        if let Some(audit) = self.audit {
            if let Some(level) = audit.level {
                if audit_level_in_yaml {
                    tracing::warn!(
                        target: "pacquet::config",
                        r#"Both the "audit" and "auditLevel" settings are set. The deprecated "auditLevel" setting is ignored in favor of "audit"."#,
                    );
                }
                config.audit_level = Some(level);
            }
            if let Some(ignore) = audit.ignore {
                if audit_config_in_yaml {
                    tracing::warn!(
                        target: "pacquet::config",
                        r#"Both the "audit" and "auditConfig" settings are set. The deprecated "auditConfig" setting is ignored in favor of "audit"."#,
                    );
                }
                config.audit_config.ignore_ghsas = ignore;
            }
            if let Some(prune) = audit.ignore_prune {
                config.audit_ignore_prune = Some(prune);
            }
        }
        if let Some(v) = self.versioning {
            config.versioning = v;
        }
        if let Some(v) = self.trust_policy_exclude {
            config.trust_policy_exclude = Some(v);
        }
        if let Some(v) = self.trust_policy_ignore_after {
            config.trust_policy_ignore_after = Some(v);
        }
    }

    /// Overlay this file's proxy keys onto the merged view and re-resolve.
    ///
    /// A key named here occupies it even when the value reads as unset —
    /// see the [`crate::proxy_keys`] module docs.
    pub(crate) fn apply_proxy_to(
        &self,
        proxy_config: &mut pnpm_network::ProxyConfig,
        keys: &mut ProxyKeys,
    ) {
        for (key, raw) in [
            (&mut keys.https_proxy, self.https_proxy.as_deref()),
            (&mut keys.http_proxy, self.http_proxy.as_deref()),
        ] {
            if let Some(raw) = raw {
                *key = ProxyValue::from_config(raw);
            }
        }
        if let Some(raw) = self.proxy.as_deref() {
            keys.legacy_proxy = ProxyValue::legacy_from_config(raw);
        }
        for (key, raw) in [
            (&mut keys.no_proxy, self.no_proxy.as_ref()),
            (&mut keys.noproxy, self.noproxy.as_ref()),
        ] {
            if let Some(raw) = raw {
                *key = ProxyValue::from_config(&no_proxy_scalar(raw));
            }
        }
        *proxy_config = keys.resolve();
    }
}

/// Flatten a `noProxy` yaml scalar into the raw string form the `.npmrc`
/// spelling of the key would carry. `true` becomes the literal token the
/// `no-proxy` parser reads as "bypass every proxy"; anything that is
/// neither a string nor `true` becomes a token that reads as unset.
fn no_proxy_scalar(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Bool(true) => "true".to_string(),
        serde_json::Value::String(value) => value.clone(),
        _ => "null".to_string(),
    }
}

fn has_env_placeholder(value: &str) -> bool {
    value
        .match_indices("${")
        .any(|(start, _)| value[start + 2..].find('}').is_some_and(|end| end > 0))
}

fn substitute_optional_string<Sys: EnvVar>(value: &mut Option<String>) {
    if let Some(value) = value {
        let (substituted, _) = env_replace_lossy::<Sys>(value);
        *value = substituted;
    }
}

fn substitute_json_string<Sys: EnvVar>(value: &mut Option<serde_json::Value>) {
    if let Some(serde_json::Value::String(value)) = value {
        let (substituted, _) = env_replace_lossy::<Sys>(value);
        *value = substituted;
    }
}

fn substitute_optional_string_map<Sys: EnvVar>(value: &mut Option<BTreeMap<String, String>>) {
    if let Some(value) = value {
        for map_value in value.values_mut() {
            let (substituted, _) = env_replace_lossy::<Sys>(map_value);
            *map_value = substituted;
        }
    }
}

/// Expands `${VAR}` in the half of each `registries` entry that carries the
/// request destination: the value of a scope route, the key of a declaration.
fn substitute_registry_entries<Sys: EnvVar>(value: &mut Option<BTreeMap<String, RegistryEntry>>) {
    let Some(map) = value.take() else { return };
    *value = Some(
        map.into_iter()
            .map(|(key, entry)| match entry {
                RegistryEntry::ScopeRoute(url) => {
                    let (substituted, _) = env_replace_lossy::<Sys>(&url);
                    (key, RegistryEntry::ScopeRoute(substituted))
                }
                RegistryEntry::Declaration(declaration) => {
                    let (substituted, _) = env_replace_lossy::<Sys>(&key);
                    (substituted, RegistryEntry::Declaration(declaration))
                }
            })
            .collect(),
    );
}

fn substitute_optional_inner_string<Sys: EnvVar>(value: &mut Option<Option<String>>) {
    if let Some(Some(value)) = value {
        let (substituted, _) = env_replace_lossy::<Sys>(value);
        *value = substituted;
    }
}

fn normalize_registry_url(registry: &str) -> String {
    if registry.ends_with('/') { registry.to_string() } else { format!("{registry}/") }
}

/// Join a `~/`-relative suffix onto the home directory the way pnpm's
/// `path.join` does: concatenate with the separator, then normalize. Node
/// treats every argument after the first as a fragment, so `Path::join` is
/// the wrong primitive here — it lets a suffix that parses as rooted
/// (`~//bin`) replace the home directory outright, which is how the tilde
/// would end up naming somewhere else entirely.
fn join_home_relative(home: &Path, relative: &str) -> PathBuf {
    let mut joined = home.as_os_str().to_os_string();
    joined.push(std::path::MAIN_SEPARATOR_STR);
    joined.push(relative);
    pnpm_fs::lexical_normalize(Path::new(&joined))
}

fn resolve(base: &Path, value: &str) -> PathBuf {
    let candidate = Path::new(value);
    if candidate.is_absolute() { candidate.to_path_buf() } else { base.join(candidate) }
}

pub(crate) fn find_workspace_manifest(start: &Path) -> Option<PathBuf> {
    let mut cursor = Some(start);
    while let Some(dir) = cursor {
        let candidate = dir.join(WORKSPACE_MANIFEST_FILENAME);
        if candidate.is_file() {
            return Some(candidate);
        }
        cursor = dir.parent();
    }
    None
}

/// Resolve the workspace root for a given starting directory — i.e. the
/// directory containing the nearest ancestor `pnpm-workspace.yaml`.
/// Returns `start` itself if no manifest is found, so callers can always
/// use the result as a resolution base.
#[must_use]
pub fn workspace_root_or(start: &Path) -> PathBuf {
    find_workspace_manifest(start)
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| start.to_path_buf())
}

pub mod registries;

#[cfg(test)]
mod tests;
