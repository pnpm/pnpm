use super::{
    AccessSpec, ByteSize, Deserialize, Ecosystem, IndexMap, Interval, LogConfig, LogFormat,
    LogLevel, Management, OciConfig, PackageAccess, PathBuf, S3Settings, ScimConfig, TarballFormat,
    UpstreamAuthFile, default_storage_string, oidc,
};

/// Disk shape of the `routes:` block.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RoutesFile {
    #[serde(default)]
    pub(super) public: Vec<PublicRouteFile>,
    #[serde(default, alias = "allowed_private_networks")]
    pub(super) allowed_private_networks: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
    /// Whether `teams:` is the roster or only the seed of one the team API
    /// edits. Omitted ⇒ `config`.
    #[serde(default, rename = "teamsManagedBy", alias = "teams_managed_by")]
    pub(super) teams_managed_by: Management,
    /// Whether the `packages:` rules and `access:` are fixed, or a starting
    /// point the admin API may change. Omitted ⇒ `config`.
    #[serde(default, rename = "rulesManagedBy", alias = "rules_managed_by")]
    pub(super) rules_managed_by: Management,
    /// The names this registry serves and accepts publishes for — its
    /// namespace — with optional per-package `access`/`publish`/`unpublish`
    /// rules as values (`{}` or null ⇒ the registry defaults). The most
    /// specific matching key wins; key order carries no meaning. Omitted ⇒
    /// every name, default rules.
    #[serde(default)]
    pub(super) packages: IndexMap<String, Option<PackageAccess>>,
    /// The format this registry serves tarballs in at its own URL. Omitted ⇒
    /// `tgz`.
    #[serde(default)]
    pub(super) tarballs: Option<TarballFormat>,
}

/// Disk shape of an `upstream:` registry — one external origin. Mirrors an
/// upstream's tuning knobs plus `public` (an anonymous, no-credential origin)
/// and `access` (which pnpr callers may reach a private one).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "registry upstream YAML configuration format is flat"
    )
)]
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
    #[serde(default, rename = "maxAge", alias = "maxage", alias = "max_age")]
    pub(super) maxage: Option<Interval>,
    #[serde(default)]
    pub(super) timeout: Option<Interval>,
    #[serde(default, rename = "maxFails", alias = "max_fails")]
    pub(super) max_fails: Option<u32>,
    #[serde(default, rename = "failTimeout", alias = "fail_timeout")]
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
    /// The format this registry serves tarballs in at its own URL. Omitted ⇒
    /// `tgz`.
    #[serde(default)]
    pub(super) tarballs: Option<TarballFormat>,
}

/// Disk shape of a `router:` registry: an ordered list of concrete registry names.
/// The first source whose declared patterns claim a package serves it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RouterFile {
    #[serde(default)]
    pub(super) sources: Vec<String>,
    /// The format this registry serves tarballs in at its own URL. Omitted ⇒
    /// `tgz`.
    #[serde(default)]
    pub(super) tarballs: Option<TarballFormat>,
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

/// Disposable proxy-cache root or settings table.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum CacheFile {
    Path(String),
    Settings(CacheSettingsFile),
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CacheSettingsFile {
    #[serde(default)]
    pub(super) path: Option<String>,
    #[serde(default, rename = "maxSize", alias = "max_size")]
    pub(super) max_size: Option<ByteSize>,
    #[serde(default, rename = "maxAge", alias = "max_age")]
    pub(super) max_age: Option<Interval>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "registry YAML configuration format is flat"
    )
)]
pub(super) struct ConfigFile {
    #[serde(default = "default_storage_string")]
    pub(super) storage: String,
    /// Disposable proxy-cache root or settings table. When omitted
    /// it defaults to a `.pnpr-cache` subdirectory of `storage`.
    #[serde(default)]
    pub(super) cache: Option<CacheFile>,
    /// Reverse-proxy addresses trusted to supply client IP headers.
    #[serde(default, rename = "trustedProxies", alias = "trusted_proxies")]
    pub(super) trusted_proxies: Vec<String>,
    /// Header name trusted to supply client IP addresses (`x-forwarded-for`, `forwarded`, `x-real-ip`).
    #[serde(default, rename = "trustedProxyHeader", alias = "trusted_proxy_header")]
    pub(super) trusted_proxy_header: Option<String>,
    /// Request processing timeout (e.g. `60s`, `1m`).
    #[serde(default, rename = "requestTimeout", alias = "request_timeout")]
    pub(super) request_timeout: Option<Interval>,
    /// Duration before a cached packument is considered stale and refetched.
    #[serde(
        default,
        rename = "packumentTtl",
        alias = "packument_ttl",
        alias = "packument_ttl_secs",
        alias = "packumentTtlSecs"
    )]
    pub(super) packument_ttl: Option<Interval>,
    /// pnpr-only browser access policy. Cross-origin access stays disabled
    /// when this block is absent or its allowlist is empty.
    #[serde(default)]
    pub(super) cors: CorsFile,
    /// pnpr-only web UI settings.
    #[serde(default)]
    pub(super) ui: UiFile,
    #[serde(default)]
    pub(super) oci: OciConfig,
    /// pnpr-only block: store the hosted (published) packages in an
    /// S3-compatible object store instead of `storage`.
    #[serde(default)]
    pub(super) s3: Option<S3Settings>,
    /// pnpr-only block: back the auth record stores (users + tokens)
    /// with a shared SQL database.
    #[serde(default)]
    pub(super) backend: Option<BackendFile>,
    /// pnpr-only local OSV database settings.
    #[serde(default)]
    pub(super) osv: OsvFile,
    /// pnpr-only feature toggle for the resolver surface. On unless
    /// explicitly disabled. `Option` so a bare `resolver:` (which YAML
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
    /// exposed at `/~<name>/`.
    #[serde(default)]
    pub(super) registries: IndexMap<String, RegistryGroupFile>,
    /// The registry the path-less base URL aliases. Absent ⇒ the bare host has no
    /// registry and clients must address a `/~<name>/`.
    #[serde(default, rename = "defaultRegistry", alias = "default_registry")]
    pub(super) default_registry: Option<DefaultRegistryFile>,
    /// The removed top-level ACL block, kept only to *reject* it loudly.
    #[serde(default, deserialize_with = "detect_removed_block")]
    pub(super) packages: Option<RemovedPackagesBlock>,
    /// The removed top-level `groups:` block, kept only to *reject* it
    /// loudly.
    #[serde(default, deserialize_with = "detect_removed_block")]
    pub(super) groups: Option<RemovedGroupsBlock>,
    /// The unsupported top-level `uplinks:` block, kept only to *reject* it
    /// loudly.
    #[serde(default, deserialize_with = "detect_removed_block")]
    pub(super) uplinks: Option<RemovedUplinksBlock>,
    /// The removed top-level `upstreams:` block, kept only to *reject* it
    /// loudly.
    #[serde(default, deserialize_with = "detect_removed_block")]
    pub(super) upstreams: Option<RemovedUpstreamsBlock>,
    /// pnpr-only: which fetch routes the resolution cache treats as
    /// public.
    #[serde(default)]
    pub(super) routes: Option<RoutesFile>,
    /// Master secret from which purpose-specific subkeys (cache HMAC, OCI token signing)
    /// are derived when individual secrets are not configured. A random per-process
    /// secret is generated when absent.
    #[serde(default)]
    pub(super) secret: Option<String>,
    /// Optional purpose-specific secrets for independent rotation.
    #[serde(default)]
    pub(super) secrets: Option<SecretsFile>,
    #[serde(default)]
    pub(super) auth: AuthFile,
    #[serde(default)]
    pub(super) log: Option<LogEntryFile>,
}

/// Purpose-specific secrets allowing independent rotation of cache and OCI signing keys.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SecretsFile {
    #[serde(default)]
    pub(super) cache: Option<String>,
    #[serde(default)]
    pub(super) oci: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CorsFile {
    #[serde(default, alias = "allowed_origins")]
    pub(super) allowed_origins: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct UiFile {
    pub(super) enabled: bool,
    pub(super) dir: Option<String>,
}

impl Default for UiFile {
    fn default() -> Self {
        Self { enabled: true, dir: None }
    }
}

/// Marker for a present top-level `packages:` key, whatever its value.
#[derive(Debug, Default)]
pub(super) struct RemovedPackagesBlock;

/// Marker for a present top-level `groups:` key, whatever its value.
#[derive(Debug, Default)]
pub(super) struct RemovedGroupsBlock;

/// Marker for a present top-level `uplinks:` key, whatever its value.
#[derive(Debug, Default)]
pub(super) struct RemovedUplinksBlock;

/// Marker for a present top-level `upstreams:` key, whatever its value.
#[derive(Debug, Default)]
pub(super) struct RemovedUpstreamsBlock;

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

/// The YAML `log:` object.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AuthFile {
    #[serde(default)]
    pub(super) admins: Vec<String>,
    #[serde(default)]
    pub(super) oidc: Vec<oidc::OidcProvider>,
    #[serde(default)]
    pub(super) htpasswd: HtpasswdFile,
    #[serde(default)]
    pub(super) tokens: TokensFile,
    #[serde(default)]
    pub(super) scim: Option<ScimConfig>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct BackendFile {
    #[serde(default)]
    pub(super) libsql: Option<LibsqlFile>,
    #[serde(default, alias = "postgresql")]
    pub(super) postgres: Option<SqlBackendFile>,
    #[serde(default)]
    pub(super) mysql: Option<SqlBackendFile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct LibsqlFile {
    pub(super) url: String,
    #[serde(default, rename = "authToken", alias = "auth_token")]
    pub(super) auth_token: Option<String>,
    #[serde(default, rename = "replicaPath", alias = "replica_path")]
    pub(super) replica_path: Option<PathBuf>,
    #[serde(
        default,
        rename = "syncInterval",
        alias = "sync_interval",
        alias = "syncIntervalSecs",
        alias = "sync_interval_secs"
    )]
    pub(super) sync_interval: Option<Interval>,
    #[serde(default, rename = "maxConnections", alias = "max_connections")]
    pub(super) max_connections: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SqlBackendFile {
    pub(super) url: String,
    #[serde(default, rename = "maxConnections", alias = "max_connections")]
    pub(super) max_connections: Option<u32>,
    #[serde(default)]
    pub(super) timeout: Option<Interval>,
    #[serde(default, rename = "startupTimeout", alias = "startup_timeout")]
    pub(super) startup_timeout: Option<Interval>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct HtpasswdFile {
    #[serde(default)]
    pub(super) file: Option<String>,
    /// `i64` so the sentinel `-1` (registration disabled)
    /// parses; anything `≥ 0` becomes a hard cap.
    #[serde(default, rename = "maxUsers", alias = "max_users")]
    pub(super) max_users: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct TokensFile {
    #[serde(default)]
    pub(super) file: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct OsvFile {
    #[serde(default)]
    pub(super) enabled: bool,
    #[serde(default)]
    pub(super) path: Option<String>,
}

/// Disk shape of the `resolver:` feature block. The field and the whole-block
/// defaults are both `enabled: true`, so omitting the block — or writing
/// `resolver:` with no body — keeps the surface on.
/// `deny_unknown_fields` so a typo like `resolver: { enable: false }`
/// (note: `enable`, not `enabled`) is a loud config error rather than
/// silently leaving the surface enabled — the toggle scopes which
/// endpoints are exposed, so a silent default-on is a security footgun.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ArtifactsFeatureFile {
    #[serde(default)]
    pub(super) enabled: bool,
    #[serde(default)]
    pub(super) orgs: IndexMap<String, StorageAccessFile>,
    #[serde(default)]
    pub(super) quota: ArtifactQuotaFile,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ArtifactQuotaFile {
    #[serde(rename = "ownerGiB", alias = "owner_gib")]
    pub(super) owner_gib: Option<u64>,
    #[serde(rename = "totalGiB", alias = "total_gib")]
    pub(super) total_gib: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct StorageAccessFile {
    pub(super) access: AccessSpec,
    pub(super) publish: AccessSpec,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PipelineFeatureFile {
    #[serde(default)]
    pub(super) enabled: bool,
}

pub(super) fn default_true() -> bool {
    true
}
