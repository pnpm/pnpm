use super::{
    AccessSpec, Deserialize, Ecosystem, IndexMap, Interval, LibsqlSettings, LogConfig, LogFormat,
    LogLevel, OciConfig, PackageAccess, RegistryError, S3Settings, SystemEnv, UpstreamAuthFile,
    default_storage_string, env_replace_lossy, oidc,
};

/// Disk shape of the `routes:` block.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RoutesFile {
    #[serde(default)]
    pub(super) public: Vec<PublicRouteFile>,
}

#[derive(Debug, Deserialize)]
pub(super) struct PublicRouteFile {
    #[serde(default)]
    pub(super) registry: Option<String>,
    #[serde(default)]
    pub(super) package: Option<String>,
}

/// Disk shape of one `registries:` entry, discriminated by an internal `type:` tag
/// (`hosted` / `upstream` / `router`). The tag selects exactly one kind, so
/// "declared none or more than one" is unrepresentable — no runtime count check.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub(super) enum RegistryFile {
    Hosted(HostedFile),
    // Boxed: `UpstreamFile` is far larger than the other kinds, so an unboxed
    // variant would bloat every `RegistryFile`.
    Upstream(Box<UpstreamFile>),
    Router(RouterFile),
}

/// Disk shape of a `hosted:` registry.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HostedFile {
    /// The package ecosystem this registry serves, which selects the protocol
    /// spoken at its `/~<name>/` endpoint. Omitted ⇒ `npm`.
    #[serde(default)]
    pub(super) ecosystem: Option<Ecosystem>,
    /// Storage namespace for this registry's packages, so two hosted registries can
    /// hold the same `name@version` without colliding. A grouped registry defaults
    /// to `ecosystem~name`; a flat entry defaults to the storage root (`""`).
    #[serde(default)]
    pub(super) org: Option<String>,
    /// The registry-level default: who may read this registry's packages when
    /// no `packages:` entry refines it. Omitted ⇒ `$all`.
    #[serde(default)]
    pub(super) access: Option<AccessSpec>,
    /// This registry's teams: each key is a team name, each value the list
    /// of member usernames. Referenced from this registry's access lists as
    /// `team:<name>` — teams are registry-scoped, never shared across
    /// registries (YAML anchors cover a shared roster in one file).
    #[serde(default)]
    pub(super) teams: IndexMap<String, AccessSpec>,
    /// The names this registry serves and accepts publishes for — its
    /// namespace — with optional per-package `access`/`publish`/`unpublish`
    /// rules as values (`{}` or null ⇒ the registry defaults). The most
    /// specific matching key wins; key order carries no meaning. Omitted ⇒
    /// every name, default rules.
    #[serde(default)]
    pub(super) packages: IndexMap<String, Option<PackageAccess>>,
}

/// Disk shape of an `upstream:` registry — one external origin. Mirrors an
/// upstream's tuning knobs plus `public` (an anonymous, no-credential origin)
/// and `access` (which pnpr callers may reach a private one).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct UpstreamFile {
    /// The package ecosystem the origin serves, which selects the protocol
    /// pnpr proxies at `/~<name>/` and speaks to `url`. Omitted ⇒ `npm`. For
    /// `cargo`, `url` is a sparse index root (`https://index.crates.io/`); for
    /// `pypi`, a Simple API root (`https://pypi.org/simple/`).
    #[serde(default)]
    pub(super) ecosystem: Option<Ecosystem>,
    pub(super) url: String,
    /// An anonymous, world-readable origin (e.g. the public npm registry).
    /// Mutually exclusive with `auth`.
    #[serde(default)]
    pub(super) public: bool,
    #[serde(default)]
    pub(super) auth: Option<UpstreamAuthFile>,
    #[serde(default)]
    pub(super) headers: IndexMap<String, String>,
    #[serde(default)]
    pub(super) maxage: Option<Interval>,
    #[serde(default)]
    pub(super) timeout: Option<Interval>,
    #[serde(default)]
    pub(super) max_fails: Option<u32>,
    #[serde(default)]
    pub(super) fail_timeout: Option<Interval>,
    #[serde(default)]
    pub(super) cache: Option<bool>,
    /// Opt an upstream into browser-facing search and organization discovery.
    #[serde(default)]
    pub(super) search: bool,
    /// Which pnpr callers may reach this registry at `/~<name>/`. Required for a
    /// non-`public` upstream (otherwise no one could be authorized to use it).
    #[serde(default)]
    pub(super) access: Option<AccessSpec>,
    /// This registry's teams, exactly as on a hosted registry — referenced
    /// from `access` and the per-package refinements as `team:<name>`.
    #[serde(default)]
    pub(super) teams: IndexMap<String, AccessSpec>,
    /// The names that may be requested through this registry — its namespace —
    /// with optional per-package `access` refinements as values (`{}` or null
    /// ⇒ the registry default; a `publish`/`unpublish` value is a config
    /// error, since no write can land on an upstream). Omitted ⇒ every name.
    /// Bounding a private upstream stops an authorized caller from pulling
    /// arbitrary public names through its server-owned credential.
    #[serde(default)]
    pub(super) packages: IndexMap<String, Option<PackageAccess>>,
}

/// Disk shape of a `router:` registry: an ordered list of concrete registry names.
/// The first source whose declared patterns claim a package serves it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RouterFile {
    #[serde(default)]
    pub(super) sources: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(
    untagged,
    expecting = "a registry with type hosted, upstream, or router, or an ecosystem group of registries"
)]
pub(super) enum RegistryGroupFile {
    Registry(RegistryFile),
    Ecosystem(IndexMap<String, RegistryFile>),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum DefaultRegistryFile {
    Shared(String),
    Ecosystems(IndexMap<Ecosystem, String>),
}

/// Disk shape of the YAML file. Fields verdaccio supports but
/// pnpr doesn't (`auth`, `web`, `plugins`, `middlewares`,
/// `logs`, `secret`) are accepted and silently dropped via
/// `#[serde(default)]` on the fields we care about plus
/// `#[serde(deny_unknown_fields)]` *not* being set — so the same
/// `config.yaml` works for both servers.
#[derive(Debug, Deserialize)]
pub(super) struct ConfigFile {
    #[serde(default = "default_storage_string")]
    pub(super) storage: String,
    /// Disposable proxy-cache root. Not a verdaccio key — when omitted
    /// it defaults to a `.pnpr-cache` subdirectory of `storage`.
    #[serde(default)]
    pub(super) cache: Option<String>,
    /// pnpr-only browser access policy. Cross-origin access stays disabled
    /// when this block is absent or its allowlist is empty.
    #[serde(default)]
    pub(super) cors: CorsFile,
    #[serde(default)]
    pub(super) oci: OciConfig,
    /// pnpr-only block: store the hosted (published) packages in an
    /// S3-compatible object store instead of `storage`. Absent on a
    /// stock verdaccio config (silently ignored there).
    #[serde(default)]
    pub(super) s3: Option<S3Settings>,
    /// pnpr-only block: back the auth record stores (users + tokens)
    /// with a shared SQL database. Absent on a stock verdaccio config
    /// (silently ignored there).
    #[serde(default)]
    pub(super) backend: Option<BackendFile>,
    /// pnpr-only local OSV database settings.
    #[serde(default)]
    pub(super) osv: OsvFile,
    /// pnpr-only feature toggle for the resolver surface. On unless
    /// explicitly disabled; absent on a stock verdaccio config, so it
    /// stays enabled there. `Option` so a bare `resolver:` (which YAML
    /// parses as null) is accepted as "default" rather than failing to
    /// deserialize into the struct.
    #[serde(default)]
    pub(super) resolver: Option<FeatureFile>,
    /// pnpr-only feature toggle for signed shared artifacts. It is a peer of
    /// the resolver because deployments may mount either surface alone.
    #[serde(default)]
    pub(super) artifacts: Option<ArtifactsFeatureFile>,
    /// pnpr-only feature toggle for the pipeline run-record surface, a peer
    /// of the artifact store.
    #[serde(default)]
    pub(super) pipeline: Option<PipelineFeatureFile>,
    /// pnpr registries: hosted, upstream, and router origins, each
    /// exposed at `/~<name>/`. The only routing surface — there is no legacy
    /// `upstreams:`/`packages: proxy:` fallback.
    #[serde(default)]
    pub(super) registries: IndexMap<String, RegistryGroupFile>,
    /// The registry the path-less base URL aliases. Absent ⇒ the bare host has no
    /// registry and clients must address a `/~<name>/`.
    #[serde(default, rename = "defaultRegistry")]
    pub(super) default_registry: Option<DefaultRegistryFile>,
    /// The removed top-level ACL block, kept only to *reject* it loudly.
    /// Per-package rules live on each registry's `packages:` map now; a
    /// config still carrying the global block previously enforced access
    /// with it, so silently dropping the key (the fate of unknown verdaccio
    /// fields) would be a security regression — private packages would
    /// quietly open up on upgrade. Presence-detected through a custom
    /// deserializer because a plain `Option` maps a *bare* `packages:`
    /// (YAML null) to `None`, which would slip past the rejection.
    #[serde(default, deserialize_with = "detect_removed_block")]
    pub(super) packages: Option<RemovedPackagesBlock>,
    /// The removed top-level `groups:` block, kept only to *reject* it
    /// loudly. Teams are declared per registry (`registries.<name>.teams`)
    /// and referenced as `team:<name>`; a config still carrying the global
    /// block previously granted access through its group names, so it must
    /// be migrated, not silently dropped.
    #[serde(default, deserialize_with = "detect_removed_block")]
    pub(super) groups: Option<RemovedGroupsBlock>,
    /// pnpr-only: which fetch routes the resolution cache treats as
    /// public. Absent on a stock verdaccio config (built-in defaults
    /// apply).
    #[serde(default)]
    pub(super) routes: Option<RoutesFile>,
    /// Verdaccio's `secret:` — reused here to key the private
    /// resolution-cache HMAC. A random per-process secret is used when
    /// absent.
    #[serde(default)]
    pub(super) secret: Option<String>,
    #[serde(default)]
    pub(super) auth: AuthFile,
    /// Verdaccio 6+ shape: `log:` is a single object at the top
    /// level, not a list. The older `logs:` list shape is
    /// intentionally not accepted.
    #[serde(default)]
    pub(super) log: Option<LogEntryFile>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CorsFile {
    #[serde(default)]
    pub(super) allowed_origins: Vec<String>,
}

/// Marker for a present top-level `packages:` key, whatever its value.
#[derive(Debug, Default)]
pub(super) struct RemovedPackagesBlock;

/// Marker for a present top-level `groups:` key, whatever its value.
#[derive(Debug, Default)]
pub(super) struct RemovedGroupsBlock;

/// `Some` whenever the key is present — including a key with no value
/// (YAML null), which `Option<IgnoredAny>` would map to `None` and let
/// slip past the loud rejection. The value itself is consumed and
/// discarded; only presence matters.
pub(super) fn detect_removed_block<'de, De, Marker>(
    deserializer: De,
) -> Result<Option<Marker>, De::Error>
where
    De: serde::Deserializer<'de>,
    Marker: Default,
{
    serde::de::IgnoredAny::deserialize(deserializer)?;
    Ok(Some(Marker::default()))
}

/// The YAML `log:` object. Mirrors verdaccio 6's logger config.
#[derive(Debug, Deserialize)]
pub(super) struct LogEntryFile {
    #[serde(default = "default_log_type")]
    pub(super) r#type: String,
    #[serde(default)]
    pub(super) format: Option<LogFormat>,
    #[serde(default)]
    pub(super) level: Option<LogLevel>,
}

pub(super) fn default_log_type() -> String {
    LogConfig::STDOUT_SINK.to_string()
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct AuthFile {
    #[serde(default)]
    pub(super) oidc: Vec<oidc::OidcProvider>,
    #[serde(default)]
    pub(super) htpasswd: HtpasswdFile,
    #[serde(default)]
    pub(super) tokens: TokensFile,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct BackendFile {
    #[serde(default)]
    pub(super) libsql: Option<LibsqlSettings>,
    #[serde(default)]
    pub(super) postgres: Option<SqlBackendFile>,
    #[serde(default)]
    pub(super) postgresql: Option<SqlBackendFile>,
    #[serde(default)]
    pub(super) mysql: Option<SqlBackendFile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SqlBackendFile {
    pub(super) url: String,
    #[serde(default)]
    pub(super) max_connections: Option<u32>,
    #[serde(default)]
    pub(super) timeout: Option<Interval>,
    #[serde(default)]
    pub(super) startup_timeout: Option<Interval>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct HtpasswdFile {
    #[serde(default)]
    pub(super) file: Option<String>,
    /// `i64` so the verdaccio sentinel `-1` (registration disabled)
    /// parses; anything `≥ 0` becomes a hard cap.
    #[serde(default)]
    pub(super) max_users: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct TokensFile {
    #[serde(default)]
    pub(super) file: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct OsvFile {
    #[serde(default)]
    pub(super) enabled: bool,
    #[serde(default)]
    pub(super) path: Option<String>,
}

/// Disk shape of the `resolver:` feature block. A bare `enabled` today;
/// sub-feature keys can be added later. The field and the whole-block
/// defaults are both `enabled: true`, so omitting the block — or writing
/// `resolver:` with no body — keeps the surface on.
/// `deny_unknown_fields` so a typo like `resolver: { enable: false }`
/// (note: `enable`, not `enabled`) is a loud config error rather than
/// silently leaving the surface enabled — the toggle scopes which
/// endpoints are exposed, so a silent default-on is a security footgun.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FeatureFile {
    #[serde(default = "default_true")]
    pub(super) enabled: bool,
}

impl Default for FeatureFile {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ArtifactsFeatureFile {
    #[serde(default)]
    pub(super) enabled: bool,
    #[serde(default, rename = "compilerCaches")]
    pub(super) compiler_caches: IndexMap<String, StorageAccessFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StorageAccessFile {
    pub(super) access: AccessSpec,
    pub(super) publish: AccessSpec,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PipelineFeatureFile {
    #[serde(default)]
    pub(super) enabled: bool,
    #[serde(default)]
    pub(super) workspaces: IndexMap<String, StorageAccessFile>,
}

pub(super) fn default_true() -> bool {
    true
}

pub(super) fn parse_config_file(raw: &str) -> Result<ConfigFile, RegistryError> {
    let (substituted, unresolved) = env_replace_lossy::<SystemEnv>(raw);
    if !unresolved.is_empty() {
        tracing::warn!(?unresolved, "config references unset environment variables");
    }
    let file: ConfigFile = serde_saphyr::from_str(&substituted)
        .map_err(|err| RegistryError::InvalidConfig { reason: err.to_string() })?;
    if file.oci.max_blob_bytes == 0 || file.oci.max_manifest_bytes == 0 {
        return Err(RegistryError::InvalidConfig {
            reason: "oci size limits must be greater than zero".to_string(),
        });
    }
    Ok(file)
}

/// The global ACL and group blocks are gone, not ignorable: they used
/// to *enforce* and *grant* access, so dropping either like an unknown
/// verdaccio key would silently change who may reach what on upgrade.
/// Fail loudly instead, naming the replacement.
pub(super) fn reject_removed_blocks(
    has_packages: bool,
    has_groups: bool,
) -> Result<(), RegistryError> {
    if has_packages {
        return Err(RegistryError::InvalidConfig {
            reason: "the top-level `packages:` block was removed: declare per-package rules \
                     on the registry that serves them, as `registries.<name>.packages` \
                     (pattern keys, `access`/`publish`/`unpublish` values)"
                .to_string(),
        });
    }
    if has_groups {
        return Err(RegistryError::InvalidConfig {
            reason: "the top-level `groups:` block was removed: declare teams on the \
                     registry that uses them, as `registries.<name>.teams` (team-name keys, \
                     member-list values), and reference them from that registry's access \
                     lists as `team:<name>`"
                .to_string(),
        });
    }
    Ok(())
}
