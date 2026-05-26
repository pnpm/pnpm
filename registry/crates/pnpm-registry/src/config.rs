use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use indexmap::IndexMap;
use serde::Deserialize;

use crate::policy::PackagePolicies;

/// The bundled verdaccio-shaped YAML config, mirrored from
/// `@pnpm/registry-mock`'s `registry/config.yaml`. Other crates can
/// pull this in directly when they need pnpm-registry's defaults
/// (uplinks, package routing) without reading a file from disk —
/// e.g. test mocks that want to run with the standard `**` -> `npmjs`
/// routing applied.
pub const DEFAULT_CONFIG_YAML: &str = include_str!("../config.yaml");

/// Runtime configuration for the pnpm registry server.
///
/// The persisted (YAML) shape follows verdaccio's `config.yaml` —
/// `storage`, `uplinks`, `packages` — restricted to the subset
/// pnpm-registry implements (no web UI, auth, plugins, or logs
/// routing).
///
/// Runtime-only fields (`listen`, `public_url`, `packument_ttl`)
/// are set by the binary's CLI flags after the YAML is loaded,
/// matching verdaccio's CLI overrides.
#[derive(Debug, Clone)]
pub struct Config {
    /// Address the HTTP server binds to.
    pub listen: SocketAddr,
    /// URL clients should use to reach this server. Used to rewrite
    /// `dist.tarball` URLs in served packuments so tarball requests
    /// flow through this server.
    pub public_url: String,
    /// Directory under which packuments and tarballs live.
    /// In proxy mode this doubles as the cache; with no matching
    /// `proxy:` rule it is the source of truth.
    pub storage: PathBuf,
    /// Named upstream npm registries. Referenced by name from
    /// [`PackageAccess::proxy`].
    pub uplinks: IndexMap<String, UplinkConfig>,
    /// Package routing rules, evaluated in declared order. The first
    /// pattern that matches a requested package supplies its
    /// uplink (via `proxy`). Patterns without a `proxy` make the
    /// package storage-only (effectively static for that pattern).
    pub packages: IndexMap<String, PackageAccess>,
    /// How long a cached packument is considered fresh before it is
    /// re-fetched from the resolved uplink. Ignored when no uplink
    /// matches.
    pub packument_ttl: Duration,
    /// Per-package access and publish rules. Defaults to
    /// [`PackagePolicies::registry_mock_defaults`] so a vanilla
    /// `Config::proxy` / `Config::static_serve` enforces the same
    /// `@private/*` and `@pnpm.e2e/needs-auth` policies that
    /// `@pnpm/registry-mock` did under verdaccio.
    pub policies: PackagePolicies,
}

/// Verdaccio-shaped uplink declaration. Only `url` is honored —
/// other fields verdaccio supports (auth headers, timeouts, agent
/// options) are not implemented yet.
#[derive(Debug, Deserialize, Clone)]
pub struct UplinkConfig {
    pub url: String,
}

/// Per-package routing rules. `access` and `publish` are parsed for
/// config compatibility but ignored (pnpm-registry is read-only and
/// has no auth). `proxy` selects the [`UplinkConfig`] by name.
#[derive(Debug, Deserialize, Default, Clone)]
pub struct PackageAccess {
    pub access: Option<String>,
    pub publish: Option<String>,
    pub unpublish: Option<String>,
    pub proxy: Option<String>,
}

/// Disk shape of the YAML file. Fields verdaccio supports but
/// pnpm-registry doesn't (`auth`, `web`, `plugins`, `middlewares`,
/// `logs`, `secret`) are accepted and silently dropped via
/// `#[serde(default)]` on the fields we care about plus
/// `#[serde(deny_unknown_fields)]` *not* being set — so the same
/// `config.yaml` works for both servers.
#[derive(Debug, Deserialize)]
struct ConfigFile {
    #[serde(default = "default_storage_string")]
    storage: String,
    #[serde(default)]
    uplinks: IndexMap<String, UplinkConfig>,
    #[serde(default)]
    packages: IndexMap<String, PackageAccess>,
}

impl Config {
    /// Default `listen` when one isn't supplied by the caller.
    pub const DEFAULT_LISTEN: &'static str = "127.0.0.1:4873";
    /// Default packument TTL — five minutes, matching the historical
    /// proxy-mode default.
    pub const DEFAULT_PACKUMENT_TTL: Duration = Duration::from_secs(5 * 60);

    /// Build a proxy-mode config with the default npm upstream: a single
    /// `npmjs` uplink plus a `**` package rule that routes everything
    /// through it. Kept for callers that don't use YAML config.
    pub fn proxy(listen: SocketAddr, storage: PathBuf) -> Self {
        let mut uplinks = IndexMap::new();
        uplinks.insert(
            "npmjs".to_string(),
            UplinkConfig { url: "https://registry.npmjs.org".to_string() },
        );
        let mut packages = IndexMap::new();
        packages
            .insert("**".to_string(), PackageAccess { proxy: Some("npmjs".to_string()), ..Default::default() });
        Self {
            listen,
            public_url: format!("http://{listen}"),
            storage,
            uplinks,
            packages,
            packument_ttl: Self::DEFAULT_PACKUMENT_TTL,
            policies: PackagePolicies::registry_mock_defaults(),
        }
    }

    /// Build a static-mode config that serves `storage` verbatim:
    /// no uplinks declared, so no package rule resolves to one.
    pub fn static_serve(listen: SocketAddr, storage: PathBuf) -> Self {
        Self {
            listen,
            public_url: format!("http://{listen}"),
            storage,
            uplinks: IndexMap::new(),
            packages: IndexMap::new(),
            packument_ttl: Self::DEFAULT_PACKUMENT_TTL,
            policies: PackagePolicies::registry_mock_defaults(),
        }
    }

    /// Load YAML from `path` and merge it with runtime values
    /// supplied by the binary. `listen` and `public_url` are not
    /// represented in verdaccio's YAML and must be provided here;
    /// `packument_ttl` defaults to [`Self::DEFAULT_PACKUMENT_TTL`].
    ///
    /// `storage` from the YAML is resolved relative to the config
    /// file's parent directory when not absolute — same convention
    /// verdaccio uses for `./storage`.
    pub fn from_yaml(
        path: &Path,
        listen: SocketAddr,
        public_url: Option<String>,
    ) -> std::io::Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        Self::from_yaml_str(&raw, base, listen, public_url).map_err(|err| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("parse {}: {err}", path.display()),
            )
        })
    }

    /// Parse [`DEFAULT_CONFIG_YAML`] (the verdaccio-shaped YAML
    /// bundled into the binary) and merge it with the given runtime
    /// values. Relative `storage:` paths in the bundled YAML are
    /// resolved against `base_dir` — pass [`Path::new(".")`] to mirror
    /// verdaccio's CWD-relative behaviour, or an absolute path when
    /// the caller knows where the storage should live.
    ///
    /// Panics if the bundled YAML fails to parse — that would be a
    /// build-time bug since the file is compiled in.
    pub fn from_default_yaml(
        base_dir: &Path,
        listen: SocketAddr,
        public_url: Option<String>,
    ) -> Self {
        Self::from_yaml_str(DEFAULT_CONFIG_YAML, base_dir, listen, public_url)
            .expect("bundled DEFAULT_CONFIG_YAML must always parse")
    }

    fn from_yaml_str(
        raw: &str,
        base_dir: &Path,
        listen: SocketAddr,
        public_url: Option<String>,
    ) -> Result<Self, serde_saphyr::Error> {
        let file: ConfigFile = serde_saphyr::from_str(raw)?;
        let storage = resolve_relative(&file.storage, base_dir);
        let public_url = public_url.unwrap_or_else(|| format!("http://{listen}"));
        Ok(Self {
            listen,
            public_url,
            storage,
            uplinks: file.uplinks,
            packages: file.packages,
            packument_ttl: Self::DEFAULT_PACKUMENT_TTL,
            // Policies could be derived from `packages[*].{access,publish}`
            // here, but the bundled `config.yaml` already matches the
            // `registry_mock_defaults` set verbatim, and a YAML-driven
            // policy wiring is out of scope for this rebase. Keep the
            // hard-coded defaults — same as `proxy` / `static_serve`.
            policies: PackagePolicies::registry_mock_defaults(),
        })
    }

    /// Find the uplink for `package_name` by walking [`Self::packages`]
    /// in declared order and returning the first matching pattern's
    /// `proxy` -> [`Self::uplinks`] entry. The returned tuple's first
    /// element is the uplink *name* (the key in [`Self::uplinks`]);
    /// callers that have pre-built per-uplink state can use it as an
    /// index.
    pub fn resolve_uplink(&self, package_name: &str) -> Option<(&str, &UplinkConfig)> {
        let proxy_name = self.packages.iter().find_map(|(pattern, access)| {
            let proxy = access.proxy.as_deref()?;
            pattern_matches(pattern, package_name).then_some(proxy)
        })?;
        self.uplinks.get_key_value(proxy_name).map(|(k, v)| (k.as_str(), v))
    }
}

/// Resolve a (possibly relative) storage path against `base_dir`.
/// Verdaccio's `./storage` convention.
fn resolve_relative(raw: &str, base_dir: &Path) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        return path;
    }
    base_dir.join(path)
}

fn default_storage_string() -> String {
    "./storage".to_string()
}

/// Match a verdaccio package pattern against a package name.
/// Supports:
///   - `**`        — matches everything
///   - `@*/*`      — matches all scoped packages
///   - `@scope/*`  — matches every package in a specific scope
///   - exact name  — literal match
fn pattern_matches(pattern: &str, name: &str) -> bool {
    if pattern == "**" {
        return true;
    }
    if pattern == "@*/*" {
        return name.starts_with('@');
    }
    if let Some(scope) = pattern.strip_suffix("/*").and_then(|p| p.strip_prefix('@')) {
        let Some(name_scope) = name.strip_prefix('@').and_then(|n| n.split('/').next()) else {
            return false;
        };
        return name_scope == scope;
    }
    pattern == name
}
