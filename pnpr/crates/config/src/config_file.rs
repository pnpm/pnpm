use super::{
    AccessSpec, ByteSize, Deserialize, Ecosystem, GcFile, IndexMap, Interval, LogFormat, LogLevel,
    MetricsConfig, OciFile, PackageAccess, PackageDocumentsFile, PathBuf, ReleaseRecordsFile,
    ScimConfig, Serialize, TarballFormat, TrustedProxyHeader, UpstreamAuthFile, access::TeamFile,
    null_default::deserialize_null_default, oidc, storage_config::StorageFile,
};

/// Disk shape of the `routes:` block.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RoutesFile {
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub(super) public: Vec<PublicRouteFile>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub(super) allowed_private_networks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PublicRouteFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) registry: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) package: Option<String>,
}

/// Disk shape of one `registries:` entry, discriminated by an internal `type:` tag
/// (`hosted` / `upstream` / `router`). The tag selects exactly one kind, so
/// "declared none or more than one" is unrepresentable — no runtime count check.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub(super) enum RegistryFile {
    Hosted(HostedFile),
    // Boxed: `UpstreamFile` is far larger than the other kinds, so an unboxed
    // variant would bloat every `RegistryFile`.
    Upstream(Box<UpstreamFile>),
    Router(RouterFile),
}

/// Disk shape of a `hosted:` registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct HostedFile {
    /// The package ecosystem this registry serves, which selects the protocol
    /// spoken at its `/~<name>/` endpoint. Omitted ⇒ `npm`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) ecosystem: Option<Ecosystem>,
    /// Storage namespace for this registry's packages, so two hosted registries can
    /// hold the same `name@version` without colliding. Omitted ⇒ the default
    /// namespace (`""`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) namespace: Option<String>,
    /// The registry-level default: who may read this registry's packages when
    /// no `packages:` entry refines it. Omitted ⇒ `$all`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) access: Option<AccessSpec>,
    /// This registry's teams: each key is a team ID, each value its name
    /// and member account IDs. Referenced from this registry's access lists as
    /// `team:<id>` — teams are registry-scoped, never shared across
    /// registries (YAML anchors cover a shared roster in one file).
    #[serde(
        default,
        deserialize_with = "deserialize_null_default",
        skip_serializing_if = "IndexMap::is_empty"
    )]
    pub(super) teams: IndexMap<String, TeamFile>,
    /// The package patterns this registry serves and accepts publishes for,
    /// with optional per-package `access`/`publish`/`unpublish`
    /// rules as values (`{}` or null ⇒ the registry defaults). The most
    /// specific matching key wins; key order carries no meaning. Omitted ⇒
    /// every name, default rules.
    #[serde(
        default,
        deserialize_with = "deserialize_null_default",
        skip_serializing_if = "IndexMap::is_empty"
    )]
    pub(super) packages: IndexMap<String, Option<PackageAccess>>,
    /// The format this registry serves tarballs in at its own URL. Omitted ⇒
    /// `tgz`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) tarballs: Option<TarballFormat>,
}

/// Disk shape of an `upstream:` registry — one external origin. Mirrors an
/// upstream's tuning knobs plus `public` (an anonymous, no-credential origin)
/// and `access` (which pnpr callers may reach a private one).
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) ecosystem: Option<Ecosystem>,
    pub(super) url: String,
    /// An anonymous, world-readable origin (e.g. the public npm registry).
    /// Mutually exclusive with `auth`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub(super) public: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) auth: Option<UpstreamAuthFile>,
    #[serde(
        default,
        deserialize_with = "deserialize_null_default",
        skip_serializing_if = "IndexMap::is_empty"
    )]
    pub(super) headers: IndexMap<String, crate::Secret>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) packument_ttl: Option<Interval>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) timeout: Option<Interval>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) max_packument_size: Option<ByteSize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) max_fails: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) fail_timeout: Option<Interval>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) cache: Option<bool>,
    /// Opt an upstream into browser-facing search and organization discovery.
    #[serde(default, skip_serializing_if = "is_false")]
    pub(super) search: bool,
    /// Which pnpr callers may reach this registry at `/~<name>/`. Required for a
    /// non-`public` upstream (otherwise no one could be authorized to use it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) access: Option<AccessSpec>,
    /// This registry's teams, exactly as on a hosted registry — referenced
    /// from `access` and the per-package refinements as `team:<id>`.
    #[serde(
        default,
        deserialize_with = "deserialize_null_default",
        skip_serializing_if = "IndexMap::is_empty"
    )]
    pub(super) teams: IndexMap<String, TeamFile>,
    /// The package patterns that may be requested through this registry,
    /// with optional per-package `access` refinements as values (`{}` or null
    /// ⇒ the registry default; a `publish`/`unpublish` value is a config
    /// error, since no write can land on an upstream). Omitted ⇒ every name.
    /// Bounding a private upstream stops an authorized caller from pulling
    /// arbitrary public names through its server-owned credential.
    #[serde(
        default,
        deserialize_with = "deserialize_null_default",
        skip_serializing_if = "IndexMap::is_empty"
    )]
    pub(super) packages: IndexMap<String, Option<PackageAccess>>,
    /// The format this registry serves tarballs in at its own URL. Omitted ⇒
    /// `tgz`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) tarballs: Option<TarballFormat>,
}

/// Disk shape of a `router:` registry: an ordered list of concrete registry names.
/// The first source whose declared patterns claim a package serves it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RouterFile {
    #[serde(
        default,
        deserialize_with = "deserialize_null_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub(super) sources: Vec<String>,
    /// The format this registry serves tarballs in at its own URL. Omitted ⇒
    /// `tgz`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) tarballs: Option<TarballFormat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CacheSettingsFile {
    #[serde(default)]
    pub(super) path: Option<String>,
    #[serde(default)]
    pub(super) max_size: Option<ByteSize>,
    #[serde(default)]
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
    /// The address the server binds to. The `--listen` flag overrides it.
    #[serde(default)]
    pub(super) listen: Option<std::net::SocketAddr>,
    /// The URL clients reach the server at. The `--public-url` flag
    /// overrides it.
    #[serde(default)]
    pub(super) public_url: Option<String>,
    /// Where pnpr keeps its authoritative data: the storage directory, the
    /// hosted store, and the database.
    #[serde(default)]
    pub(super) storage: Option<StorageFile>,
    /// Disposable proxy-cache root or settings table. When omitted
    /// it defaults to a `.pnpr-cache` subdirectory of the storage directory.
    #[serde(default)]
    pub(super) cache: Option<CacheFile>,
    /// Reverse proxies trusted to supply the client IP address.
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub(super) trusted_proxies: TrustedProxiesFile,
    /// Request processing timeout (e.g. `60s`, `1m`).
    #[serde(default)]
    pub(super) request_timeout: Option<Interval>,
    /// Graceful shutdown drain timeout (e.g. `25s`, `1m`).
    #[serde(default)]
    pub(super) shutdown_timeout: Option<Interval>,
    /// Duration before a cached packument is considered stale and refetched.
    #[serde(default)]
    pub(super) packument_ttl: Option<Interval>,
    /// The largest tarball pnpr fetches from an upstream registry (e.g.
    /// `100MiB`).
    #[serde(default)]
    pub(super) max_tarball_size: Option<ByteSize>,
    /// pnpr-only: how pnpr's own package document is served.
    #[serde(default)]
    pub(super) package_documents: Option<PackageDocumentsFile>,
    #[serde(default)]
    pub(super) cloudflare_cdn: Option<super::CloudflareCdnConfig>,
    /// pnpr-only: how the release records of hosted versions are logged.
    #[serde(default)]
    pub(super) release_records: Option<ReleaseRecordsFile>,
    /// pnpr-only: how `pnpr gc` reclaims unreferenced archives.
    #[serde(default)]
    pub(super) gc: Option<GcFile>,
    /// pnpr-only browser access policy. Cross-origin access stays disabled
    /// when this block is absent or its allowlist is empty.
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub(super) cors: CorsFile,
    /// pnpr-only web UI settings.
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub(super) ui: UiFile,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub(super) oci: OciFile,
    /// Who may scrape Prometheus metrics. Omitted ⇒ pnpr serves none.
    #[serde(default)]
    pub(super) metrics: Option<MetricsConfig>,
    /// pnpr-only local OSV database settings.
    #[serde(default, deserialize_with = "deserialize_null_default")]
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
    /// Registries belong in the database; the key is read only to
    /// refuse it with a pointer to `pnpr registries import`.
    #[serde(default)]
    pub(super) registries: Option<serde::de::IgnoredAny>,
    /// Refused like [`Self::registries`].
    #[serde(default)]
    pub(super) default_registry: Option<serde::de::IgnoredAny>,
    /// The environment variables a registry graph stored in the database may
    /// reference.
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub(super) registry_env: Vec<String>,
    /// pnpr-only: which fetch routes the resolution cache treats as
    /// public.
    #[serde(default)]
    pub(super) routes: Option<RoutesFile>,
    /// The roots the cache and OCI keys derive from. A random per-process
    /// root stands in for an absent one.
    #[serde(default)]
    pub(super) secret: Option<crate::secrets::SecretFile>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub(super) auth: AuthFile,
    #[serde(default)]
    pub(super) log: Option<LogEntryFile>,
}

/// Disk shape of the `trustedProxies:` block.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct TrustedProxiesFile {
    /// Addresses and CIDR networks of the proxies.
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub(super) networks: Vec<String>,
    /// The header the proxies put the client address in.
    #[serde(default)]
    pub(super) header: Option<TrustedProxyHeader>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CorsFile {
    #[serde(default, deserialize_with = "deserialize_null_default")]
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

/// The YAML `log:` object.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct LogEntryFile {
    #[serde(default)]
    pub(super) format: Option<LogFormat>,
    #[serde(default)]
    pub(super) level: Option<LogLevel>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AuthFile {
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub(super) admins: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub(super) registry_admins: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub(super) oidc: Vec<oidc::OidcProvider>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub(super) users: UsersFile,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub(super) tokens: TokensFile,
    #[serde(default)]
    pub(super) scim: Option<ScimConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct LibsqlFile {
    pub(crate) url: String,
    #[serde(default)]
    pub(crate) auth_token: Option<String>,
    #[serde(default)]
    pub(crate) replica_path: Option<PathBuf>,
    #[serde(default)]
    pub(crate) sync_interval: Option<Interval>,
    #[serde(default)]
    pub(crate) max_connections: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct UsersFile {
    /// `i64` so the sentinel `-1` (registration disabled)
    /// parses; anything `≥ 0` becomes a hard cap.
    #[serde(default)]
    pub(super) max_users: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct TokensFile {
    #[serde(default)]
    pub(super) default_lifetime: Option<Interval>,
    #[serde(default)]
    pub(super) max_lifetime: Option<Interval>,
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
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub(super) owners: IndexMap<String, StorageAccessFile>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub(super) quota: ArtifactQuotaFile,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ArtifactQuotaFile {
    #[serde(default)]
    pub(super) per_owner: Option<ByteSize>,
    #[serde(default)]
    pub(super) total: Option<ByteSize>,
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

/// `skip_serializing_if` for a flag that defaults to `false`.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's `skip_serializing_if` passes the field by reference"
)]
pub(super) fn is_false(value: &bool) -> bool {
    !*value
}
