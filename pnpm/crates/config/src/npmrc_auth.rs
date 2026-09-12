pub(crate) use credential_keys::enforce_token_helper_trust;
pub(crate) use credentials::RawCreds;
pub use credentials::{BasicAuth, RegistryCreds};
pub use json::{DeclaredRegistries, is_json_auth_scope, validate_json_auth_registry};
pub(crate) use tls::parse_no_proxy;

use crate::{Config, api::EnvVar, proxy_keys::ProxyValue, workspace_yaml::LoadWorkspaceYamlError};
use indexmap::IndexMap;
use pnpm_env_replace::env_replace_lossy;
use pnpm_network::{
    AuthHeaders, DEFAULT_REGISTRY_SCOPE, NoProxySetting, PerRegistryTls, RegistryTls,
    base64_encode, base64_encode_bytes, nerf_dart,
};
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::Arc,
};

/// Subset of `.npmrc` keys pacquet honours for registry / auth setup.
///
/// The parser pulls out:
/// * the top-level `registry=` URL (already supported pre-[#336]),
/// * scoped registry routes (`@scope:registry=...`),
/// * default-registry credentials (`_auth`, `_authToken`,
///   `username` + `_password`),
/// * per-registry credentials keyed on a nerf-darted URI prefix
///   (e.g. `//npm.pkg.github.com/pnpm/:_authToken=…`),
/// * proxy keys (`https-proxy`, `http-proxy`, `proxy` legacy, and
///   `no-proxy` / `noproxy` aliases). The env-var fallback cascade
///   (`HTTPS_PROXY`, `HTTP_PROXY`, `PROXY`, `NO_PROXY` + lowercase)
///   fires from [`NpmrcAuth::apply_proxy_cascade`].
/// * TLS + `local-address` keys (`ca`, `cafile`, `cert`, `key`,
///   `strict-ssl`, `local-address`). `cafile` reads from disk and
///   feeds the same slot as inline `ca`; an unreadable `cafile` is
///   silently treated as unset.
///   Applied via [`NpmrcAuth::apply_tls_and_local_address`].
///
/// Values pass through `${VAR}` substitution before being stored.
/// Unresolved placeholders are substituted with `""` and recorded as
/// warnings so the literal `${VAR}` never reaches downstream auth code
/// (critical for OIDC trusted publishing — see
/// <https://github.com/pnpm/pnpm/issues/11513>).
///
/// Other project-structural `.npmrc` knobs remain unparsed for now.
/// They will land here as the matching feature work picks them up.
///
/// [#336]: https://github.com/pnpm/pacquet/issues/336
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct NpmrcAuth {
    pub registry: Option<String>,
    pub scoped_registries: BTreeMap<String, String>,
    /// Unscoped creds (i.e. `_auth=…`, `_authToken=…`, `username=…` /
    /// `_password=…` without a leading `//host/`), as written. Emptied by
    /// [`NpmrcAuth::rescope_unscoped`], which pins them to the registry
    /// this same source declared; nothing downstream reads them.
    pub default_creds: RawCreds,
    /// Per-registry creds keyed as `[registry_uri][scope]`. The `@`
    /// scope stores registry-wide credentials.
    pub creds_by_scope_by_uri: HashMap<String, HashMap<String, RawCreds>>,
    /// `${VAR}` placeholders that could not be resolved while parsing.
    /// Surfaced as warnings.
    pub warnings: Vec<String>,
    /// `https-proxy=…` from .npmrc. Applied by
    /// [`NpmrcAuth::apply_proxy_cascade`].
    pub https_proxy: Option<String>,
    /// `http-proxy=…` from .npmrc.
    pub http_proxy: Option<String>,
    /// Legacy `proxy=…` from .npmrc. Feeds into the `httpsProxy` slot
    /// only when `https-proxy` is unset.
    pub legacy_proxy: Option<String>,
    /// `no-proxy=…` or `noproxy=…` from .npmrc. Last write wins: a
    /// single `noProxy` slot fed by either alias.
    pub no_proxy: Option<String>,
    /// Inline `ca=…` PEM from .npmrc. Each successive `ca=` line
    /// appends to the same `Vec`, since the array form arrives as
    /// repeated keys in INI. Combined with `cafile`'s split output by
    /// [`NpmrcAuth::apply_tls_and_local_address`].
    pub ca: Vec<String>,
    /// `cafile=<path>` from .npmrc. Read at apply time, split on
    /// `-----END CERTIFICATE-----` to produce one PEM per cert.
    /// `cafile`-not-found is silently treated as unset. A relative
    /// path is resolved against the directory of the `.npmrc` that
    /// declared it (pnpm/pnpm#11726), so `pnpm --dir <proj>`
    /// from a different cwd still finds it.
    pub cafile: Option<String>,
    /// `cert=…` client certificate PEM from .npmrc.
    pub cert: Option<String>,
    /// `key=…` client private key PEM from .npmrc.
    pub key: Option<String>,
    /// `strict-ssl=…` toggle from .npmrc. `None` = unset (defaults to
    /// strict at the apply site).
    pub strict_ssl: Option<bool>,
    /// `local-address=…` outbound interface from .npmrc. Stored as a
    /// raw string here; [`NpmrcAuth::apply_tls_and_local_address`]
    /// parses it as [`std::net::IpAddr`]. An invalid address is
    /// silently dropped.
    pub local_address: Option<String>,
    /// Per-registry TLS overrides keyed by the literal `.npmrc` key
    /// prefix (`//host[:port]/path/`). Populated by `:ca`, `:cafile`,
    /// `:cert`, `:certfile`, `:key`, `:keyfile` keys. The map is
    /// preserved verbatim through to [`PerRegistryTls`] construction
    /// so lookup keys stay byte-equivalent to the keys as written.
    pub tls_by_uri: HashMap<String, RegistryTls>,
    /// Scope→URL registry routes inferred from the `_auth` **environment
    /// variable** (`"@"` → `"default"`, `"@org"` → that scope). Safe because
    /// the credential and its destination host come from the same trusted
    /// value, so repo config can't redirect the token elsewhere. The
    /// environment is the operator's channel — a CI runner pointed at a
    /// mandated proxy — so these outrank what any config file declares.
    /// Applied after workspace yaml by
    /// [`Self::apply_json_env_registries`].
    pub json_env_registries: BTreeMap<String, String>,
    /// The same routes inferred from the `_auth` of the global config
    /// **file**. That file is the user's own store rather than a mandate —
    /// it is where `pnpm login` puts a credential — so a `registry` or
    /// `registries` a config file declares, whether an `.npmrc` or a yaml,
    /// outranks these, and they fill in only what nothing else declares. See
    /// [`Self::apply_json_env_registries`].
    pub json_file_registries: BTreeMap<String, String>,
    /// Raw INI config keys (those for which
    /// [`crate::config_types::is_ini_config_key`] holds), post-`${VAR}`
    /// substitution, captured verbatim for `pnpm config get` / `list`. This
    /// is the source of [`crate::Config::raw_auth_config`]; it does not feed
    /// auth-header resolution, which reads the structured fields above.
    pub raw_ini_config: BTreeMap<String, String>,
}

/// Default registry used when a source declares credentials but no
/// `registry=` of its own to scope them to.
const DEFAULT_REGISTRY: &str = "https://registry.npmjs.org/";

impl NpmrcAuth {
    /// Collect URL-scoped registry credentials supplied through
    /// `npm_config_//…` and `pnpm_config_//…` environment variables, e.g.
    /// `npm_config_//registry.npmjs.org/:_authToken=<token>`.
    ///
    /// The registry a credential applies to is encoded in the (trusted)
    /// variable name, so — unlike a project `.npmrc` — these cannot be
    /// redirected to another host by repository-controlled config. That makes
    /// them a safe, file-free way to configure registry auth. The prefix is
    /// matched case-insensitively (as npm does); the remainder keeps its case
    /// because credential keys are case-sensitive (`:_authToken`). When the
    /// same key is set through both prefixes, `pnpm_config_` wins.
    ///
    /// Only the four credential fields (`_authToken`, `_auth`, `username`,
    /// `_password`) are honored — the same set [`split_creds_key`] recognises.
    /// Values are used verbatim (no `${VAR}` re-expansion): they already come
    /// resolved from the environment.
    pub fn from_url_scoped_env<Sys: EnvVar>() -> Self {
        // Merge into one map keyed by the URL-scoped key so each key is applied
        // once. `pnpm_config_` is extended last so it wins over `npm_config_`.
        let mut npm_scoped: HashMap<String, String> = HashMap::new();
        let mut pnpm_scoped: HashMap<String, String> = HashMap::new();
        for (name, value) in Sys::vars().into_iter().filter(|(_, value)| !value.is_empty()) {
            let Some((is_pnpm, key)) = parse_url_scoped_env_name(&name) else {
                continue;
            };
            let target = if is_pnpm { &mut pnpm_scoped } else { &mut npm_scoped };
            target.insert(key.to_owned(), value);
        }
        npm_scoped.extend(pnpm_scoped);

        let mut auth = NpmrcAuth::default();
        for (key, value) in npm_scoped {
            if let Some((uri, suffix)) = split_creds_key(&key) {
                let entry = auth.creds_entry_mut(uri);
                apply_creds_field(entry, suffix, value);
            }
        }
        auth
    }

    /// Resolve the TLS + `local-address` slots on `config.tls`.
    ///
    /// The transformations:
    /// - Inline `ca=` PEMs are kept verbatim.
    /// - `cafile=<path>` is read from disk and split on
    ///   `-----END CERTIFICATE-----`.
    ///   Inline `ca` entries appear in the final list before the
    ///   `cafile` ones — same ordering as a `ca=` line followed by a
    ///   `cafile=` line. Unreadable `cafile` is silently dropped.
    /// - `local-address` is parsed as [`std::net::IpAddr`]. An invalid
    ///   value is silently dropped.
    ///
    /// `strict_ssl`, `cert`, `key` are pass-through (no transformation).
    ///
    /// `cafile` paths arrive here already absolute — relative values
    /// were resolved against the `.npmrc`'s directory in
    /// [`NpmrcAuth::from_ini`] (pnpm/pnpm#11726).
    pub fn apply_tls_and_local_address(&mut self, config: &mut Config) {
        // Inline CA first, then file-loaded CA, so a user that
        // duplicates a cert across both ends up with it added twice.
        let mut ca = std::mem::take(&mut self.ca);
        if let Some(path) = self.cafile.take() {
            ca.extend(load_cafile(Path::new(&path)));
        }
        config.tls.ca = ca;
        config.tls.cert = self.cert.take();
        config.tls.key = self.key.take();
        config.tls.strict_ssl = self.strict_ssl.take();
        config.tls.local_address = self.local_address.take().and_then(|raw| raw.parse().ok());
        // Per-registry TLS overrides. `PerRegistryTls::from_map`
        // drops any entry whose three fields are all `None`, so the
        // lookup never returns an empty hit that would otherwise
        // suppress the top-level fallback.
        config.tls_by_uri = PerRegistryTls::from_map(std::mem::take(&mut self.tls_by_uri));
    }

    /// Fold this `.npmrc` layer's proxy keys into `config.proxy_keys`,
    /// capture the environment fallbacks, and resolve the cascade.
    ///
    /// Later layers overwrite the keys they set and re-resolve — see the
    /// [`crate::proxy_keys`] module docs for why a key, once named, is
    /// never won back by a lower-priority layer.
    ///
    /// Generic over [`EnvVar`] so cascade tests can drive every branch
    /// without mutating the process environment (no `EnvGuard` global
    /// lock).
    pub fn apply_proxy_cascade<Sys: EnvVar>(&mut self, config: &mut Config) {
        // Each proxy env var is tried in literal-, upper-, then
        // lower-case order. For the var names below the literal form is
        // already either fully upper or fully lower, so the triple
        // collapses to two real attempts.
        fn env_pair<Sys: EnvVar>(upper: &str, lower: &str) -> Option<String> {
            Sys::var(upper).or_else(|| Sys::var(lower))
        }

        let keys = &mut config.proxy_keys;
        for (key, raw) in [
            (&mut keys.https_proxy, self.https_proxy.take()),
            (&mut keys.http_proxy, self.http_proxy.take()),
            (&mut keys.no_proxy, self.no_proxy.take()),
        ] {
            if let Some(raw) = raw {
                *key = ProxyValue::from_config(&raw);
            }
        }
        if let Some(raw) = self.legacy_proxy.take() {
            keys.legacy_proxy = ProxyValue::legacy_from_config(&raw);
        }
        keys.env = crate::proxy_keys::ProxyEnv {
            https_proxy: env_pair::<Sys>("HTTPS_PROXY", "https_proxy"),
            http_proxy: env_pair::<Sys>("HTTP_PROXY", "http_proxy"),
            proxy: env_pair::<Sys>("PROXY", "proxy"),
            no_proxy: env_pair::<Sys>("NO_PROXY", "no_proxy"),
        };
        config.proxy = config.proxy_keys.resolve();
    }

    /// Phase 1: write the resolved `registry` onto `config` and emit
    /// any `${VAR}`-substitution warnings. Does *not* build
    /// `auth_headers` yet. Call [`NpmrcAuth::build_auth_headers`]
    /// after every other config layer (notably `pnpm-workspace.yaml`)
    /// has had a chance to override `registry`, so default-registry
    /// creds end up keyed at the final URL.
    ///
    /// The `registry=` and `@scope:registry=` lines the `.npmrc` files
    /// carried are recorded on `declared` as they are consumed, since
    /// once written to `config` they are indistinguishable by value from
    /// the builtin default.
    pub fn apply_registry_and_warn(
        &mut self,
        config: &mut Config,
        declared: &mut DeclaredRegistries,
    ) {
        if let Some(registry) = self.registry.take() {
            declared.registry = true;
            config.registry =
                if registry.ends_with('/') { registry } else { format!("{registry}/") };
        }
        declared.scopes.extend(self.scoped_registries.keys().cloned());
        config.registries_by_scope.append(&mut self.scoped_registries);
        for message in std::mem::take(&mut self.warnings) {
            tracing::warn!(target: "pacquet::npmrc", "{message}");
        }
    }

    /// Merge a lower-priority source under `self` (the higher-priority
    /// one). Fields already set on `self` win; `lower` fills the gaps.
    /// Per-URI credential and TLS maps merge field-by-field with the
    /// same "higher wins" rule. The merge order is
    /// `user < auth.ini < workspace`, where each later source
    /// overwrites the keys an earlier one set.
    ///
    /// Both sources must already have been through
    /// [`Self::rescope_unscoped`] so their unscoped credentials are
    /// pinned to the right registry before they are combined.
    pub fn merge_under(&mut self, lower: NpmrcAuth) {
        self.registry = self.registry.take().or(lower.registry);
        for (scope, registry) in lower.scoped_registries {
            self.scoped_registries.entry(scope).or_insert(registry);
        }
        for (scope, registry) in lower.json_env_registries {
            self.json_env_registries.entry(scope).or_insert(registry);
        }
        for (scope, registry) in lower.json_file_registries {
            self.json_file_registries.entry(scope).or_insert(registry);
        }
        for (key, value) in lower.raw_ini_config {
            self.raw_ini_config.entry(key).or_insert(value);
        }
        self.https_proxy = self.https_proxy.take().or(lower.https_proxy);
        self.http_proxy = self.http_proxy.take().or(lower.http_proxy);
        self.legacy_proxy = self.legacy_proxy.take().or(lower.legacy_proxy);
        self.no_proxy = self.no_proxy.take().or(lower.no_proxy);
        if self.ca.is_empty() {
            self.ca = lower.ca;
        }
        self.cafile = self.cafile.take().or(lower.cafile);
        self.cert = self.cert.take().or(lower.cert);
        self.key = self.key.take().or(lower.key);
        self.strict_ssl = self.strict_ssl.take().or(lower.strict_ssl);
        self.local_address = self.local_address.take().or(lower.local_address);

        for (uri, lower_by_scope) in lower.creds_by_scope_by_uri {
            let by_scope = self.creds_by_scope_by_uri.entry(uri).or_default();
            for (scope, creds) in lower_by_scope {
                by_scope.entry(scope).or_default().fill_from(creds);
            }
        }
        for (uri, tls) in lower.tls_by_uri {
            let entry = self.tls_by_uri.entry(uri).or_default();
            entry.ca = entry.ca.take().or(tls.ca);
            entry.cert = entry.cert.take().or(tls.cert);
            entry.key = entry.key.take().or(tls.key);
        }
        // Lower-priority warnings come first — they were produced while
        // reading the earlier file.
        let mut warnings = lower.warnings;
        warnings.append(&mut self.warnings);
        self.warnings = warnings;
    }

    /// Convenience wrapper that runs [`apply_registry_and_warn`],
    /// [`apply_proxy_cascade`], and [`build_auth_headers`] in one call.
    /// Used by tests and other callers that don't layer additional
    /// config sources on top of `.npmrc`. Production code in
    /// [`crate::Config::current`] inserts `pnpm-workspace.yaml` between
    /// phase 1 and phase 2 so default-registry creds key at the final
    /// URL.
    ///
    /// [`apply_registry_and_warn`]: NpmrcAuth::apply_registry_and_warn
    /// [`apply_proxy_cascade`]: NpmrcAuth::apply_proxy_cascade
    /// [`build_auth_headers`]: NpmrcAuth::build_auth_headers
    #[cfg(test)]
    pub fn apply_to<Sys: EnvVar>(mut self, config: &mut Config) {
        self.rescope_unscoped("<.npmrc>");
        self.apply_registry_and_warn(config, &mut DeclaredRegistries::default());
        self.apply_proxy_cascade::<Sys>(config);
        self.apply_tls_and_local_address(config);
        self.build_auth_headers(config).expect("valid credentials in test .npmrc");
    }
}

/// Normalize a registry URL for the purposes of nerf-darting: ensure a
/// single trailing slash.
fn normalize_registry_url(registry: &str) -> String {
    if registry.ends_with('/') { registry.to_string() } else { format!("{registry}/") }
}

#[cfg(test)]
mod tests;

mod ini;

mod json;

mod credentials;

use credentials::apply_creds_field;

mod credential_keys;
use credential_keys::{
    is_auth_value_key, is_package_scope, parse_token_helper_field, parse_url_scoped_env_name,
    split_creds_key, split_ini_creds_key, split_scope_from_uri,
};

mod tls;
use tls::{
    apply_tls_field, expand_inline_pem, load_cafile, parse_bool, resolve_cafile,
    split_inline_identity_key, split_ssl_key,
};
