use crate::{Config, api::EnvVar};
use pacquet_env_replace::env_replace_lossy;
use pacquet_network::{
    AuthHeaders, DEFAULT_REGISTRY_SCOPE, NoProxySetting, PerRegistryTls, RegistryTls, base64_encode,
};
use std::{
    collections::{BTreeMap, HashMap},
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
///   silently treated as unset (matching pnpm's
///   [`loadCAFile`](https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/loadNpmrcFiles.ts#L238-L265)).
///   Applied via [`NpmrcAuth::apply_tls_and_local_address`].
///
/// Values pass through `${VAR}` substitution before being stored,
/// matching pnpm's `loadNpmrcFiles.ts` flow. Unresolved placeholders are
/// substituted with `""` and recorded as warnings so the literal `${VAR}`
/// never reaches downstream auth code (critical for OIDC trusted publishing
/// — see <https://github.com/pnpm/pnpm/issues/11513>), again matching pnpm.
///
/// Other project-structural `.npmrc` knobs remain unparsed for now.
/// See the upstream
/// [`isIniConfigKey`](https://github.com/pnpm/pnpm/blob/601317e7a3/config/reader/src/localConfig.ts#L160-L161)
/// list. They will land here as the matching feature work picks them
/// up.
///
/// [#336]: https://github.com/pnpm/pacquet/issues/336
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct NpmrcAuth {
    pub registry: Option<String>,
    pub scoped_registries: BTreeMap<String, String>,
    /// Default-registry creds (i.e. `_auth=…`, `_authToken=…`,
    /// `username=…` / `_password=…` without a leading `//host/`).
    /// Applied to whichever URI the resolved `registry` points at.
    pub default_creds: RawCreds,
    /// Per-registry creds keyed as `[registry_uri][scope]`. The `@`
    /// scope stores registry-wide credentials.
    pub creds_by_scope_by_uri: HashMap<String, HashMap<String, RawCreds>>,
    /// `${VAR}` placeholders that could not be resolved while parsing.
    /// Surfaced as warnings; `pnpm` does the same in
    /// [`substituteEnv`](https://github.com/pnpm/pnpm/blob/601317e7a3/config/reader/src/loadNpmrcFiles.ts#L156-L162).
    pub warnings: Vec<String>,
    /// `https-proxy=…` from .npmrc. Applied by
    /// [`NpmrcAuth::apply_proxy_cascade`].
    pub https_proxy: Option<String>,
    /// `http-proxy=…` from .npmrc.
    pub http_proxy: Option<String>,
    /// Legacy `proxy=…` from .npmrc. Feeds into the `httpsProxy` slot
    /// only when `https-proxy` is unset — mirrors upstream's
    /// [`pnpmConfig.httpsProxy = pnpmConfig.proxy`](https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/index.ts#L591-L600).
    pub legacy_proxy: Option<String>,
    /// `no-proxy=…` or `noproxy=…` from .npmrc. Last write wins (matches
    /// upstream's
    /// [single `noProxy` slot](https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/index.ts#L598-L600)
    /// fed by either alias).
    pub no_proxy: Option<String>,
    /// Inline `ca=…` PEM from .npmrc. Each successive `ca=` line
    /// appends to the same `Vec` (matching upstream's
    /// `[null, String, Array]` nopt shape, where the array form
    /// arrives as repeated keys in INI). Combined with `cafile`'s
    /// split output by [`NpmrcAuth::apply_tls_and_local_address`].
    pub ca: Vec<String>,
    /// `cafile=<path>` from .npmrc. Read at apply time, split on
    /// `-----END CERTIFICATE-----` to produce one PEM per cert
    /// (mirroring pnpm's
    /// [`loadCAFile`](https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/loadNpmrcFiles.ts#L249-L255)).
    /// `cafile`-not-found is silently treated as unset. A relative
    /// path is resolved against the directory of the `.npmrc` that
    /// declared it (matching pnpm/pnpm#11726), so `pnpm --dir <proj>`
    /// from a different cwd still finds it.
    pub cafile: Option<String>,
    /// `cert=…` client certificate PEM from .npmrc.
    pub cert: Option<String>,
    /// `key=…` client private key PEM from .npmrc.
    pub key: Option<String>,
    /// `strict-ssl=…` toggle from .npmrc. `None` = unset (defaults to
    /// strict at apply site, matching pnpm's per-emit-site default at
    /// [`dispatcher.ts:191,197,241,295`](https://github.com/pnpm/pnpm/blob/94240bc046/network/fetch/src/dispatcher.ts#L191)).
    pub strict_ssl: Option<bool>,
    /// `local-address=…` outbound interface from .npmrc. Stored as a
    /// raw string here; [`NpmrcAuth::apply_tls_and_local_address`]
    /// parses it as [`std::net::IpAddr`]. An invalid address is
    /// silently dropped (mirrors pnpm, which hands the value verbatim
    /// to undici and lets Node error at connect time).
    pub local_address: Option<String>,
    /// Per-registry TLS overrides keyed by the literal `.npmrc` key
    /// prefix (`//host[:port]/path/`). Populated by `:ca`, `:cafile`,
    /// `:cert`, `:certfile`, `:key`, `:keyfile` keys. The map is
    /// preserved verbatim through to [`PerRegistryTls`] construction
    /// so lookup keys stay byte-equivalent to upstream. Mirrors
    /// pnpm's
    /// [`configByUri[<uri>].tls`](https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/getNetworkConfigs.ts#L34-L40).
    pub tls_by_uri: HashMap<String, RegistryTls>,
}

/// Raw (unparsed) credential fields for a given registry URI, mirroring
/// pnpm's
/// [`RawCreds`](https://github.com/pnpm/pnpm/blob/601317e7a3/config/reader/src/parseCreds.ts#L7-L18).
/// Each `Option` stores the post-`${VAR}`-substitution value when set.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct RawCreds {
    /// `_authToken=` value.
    pub auth_token: Option<String>,
    /// `_auth=` value (base64 of `username:password`).
    pub auth_pair_base64: Option<String>,
    /// `username=` value.
    pub username: Option<String>,
    /// `_password=` value (base64-encoded password, per npm convention).
    pub password: Option<String>,
}

#[derive(Clone, Copy)]
struct ParseOptions {
    expand_auth_value_env: bool,
    expand_request_destination_env: bool,
}

impl RawCreds {
    fn is_empty(&self) -> bool {
        self.auth_token.is_none()
            && self.auth_pair_base64.is_none()
            && self.username.is_none()
            && self.password.is_none()
    }

    /// Fill any field that is `None` here from `lower`. Used when
    /// merging a lower-priority source under a higher one: the higher
    /// source's already-set fields win, the lower fills the gaps.
    fn fill_from(&mut self, lower: RawCreds) {
        self.auth_token = self.auth_token.take().or(lower.auth_token);
        self.auth_pair_base64 = self.auth_pair_base64.take().or(lower.auth_pair_base64);
        self.username = self.username.take().or(lower.username);
        self.password = self.password.take().or(lower.password);
    }
}

/// Default registry used when a source declares credentials but no
/// `registry=` of its own to scope them to. Matches pnpm's
/// `npmDefaults.registry`.
const DEFAULT_REGISTRY: &str = "https://registry.npmjs.org/";

impl NpmrcAuth {
    pub fn from_project_ini<Sys: EnvVar>(text: &str, npmrc_dir: &Path) -> Self {
        Self::from_ini_with_options::<Sys>(
            text,
            npmrc_dir,
            ParseOptions { expand_auth_value_env: false, expand_request_destination_env: false },
        )
    }

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
        for (name, value) in Sys::vars() {
            if value.is_empty() {
                continue;
            }
            if let Some((is_pnpm, key)) = parse_url_scoped_env_name(&name) {
                let target = if is_pnpm { &mut pnpm_scoped } else { &mut npm_scoped };
                target.insert(key.to_owned(), value);
            }
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

    /// Parse an `.npmrc` file's contents and pick out the auth/network keys.
    /// Unknown keys are silently dropped. `${VAR}` placeholders inside keys
    /// and values are resolved via the [`EnvVar`] capability; unresolved
    /// placeholders (no env value and no `${VAR:-default}` fallback) are
    /// substituted with `""` and surfaced as warnings, matching pnpm's
    /// `substituteEnv` in `loadNpmrcFiles.ts`. Leaving the literal `${VAR}`
    /// in an auth value would otherwise be sent verbatim — most damagingly
    /// as a bearer auth token under OIDC trusted publishing
    /// (<https://github.com/pnpm/pnpm/issues/11513>).
    ///
    /// The `.npmrc` format is a tiny ini dialect: one `key=value` per line,
    /// plus comments starting with `;` or `#`. We hand-parse rather than
    /// use a strongly-typed deserializer so unknown / malformed keys don't
    /// blow up parsing.
    ///
    /// `npmrc_dir` is the directory of the `.npmrc` file the `text`
    /// came from. A relative `cafile=` resolves against it so a
    /// project `.npmrc` reachable via `pacquet --dir <proj>` from a
    /// different cwd still finds its CA bundle (pnpm/pnpm#11726).
    pub fn from_ini<Sys: EnvVar>(text: &str, npmrc_dir: &Path) -> Self {
        Self::from_ini_with_options::<Sys>(
            text,
            npmrc_dir,
            ParseOptions { expand_auth_value_env: true, expand_request_destination_env: true },
        )
    }

    fn from_ini_with_options<Sys: EnvVar>(
        text: &str,
        npmrc_dir: &Path,
        opts: ParseOptions,
    ) -> Self {
        let mut auth = NpmrcAuth::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            let Some((raw_key, raw_value)) = line.split_once('=') else {
                continue;
            };
            let raw_key = raw_key.trim();
            let raw_value = raw_value.trim();

            // Apply ${VAR} substitution to both the key and the value,
            // matching `readAndFilterNpmrc` in pnpm's `loadNpmrcFiles.ts`.
            // Unresolved placeholders become "" and are recorded as warnings.
            if !opts.expand_request_destination_env
                && has_env_placeholder(raw_key)
                && is_request_destination_key(raw_key)
            {
                auth.warn_ignored_request_destination_env(raw_key);
                continue;
            }
            if !opts.expand_auth_value_env
                && has_env_placeholder(raw_key)
                && is_auth_value_key(raw_key)
            {
                auth.warn_ignored_auth_value_env(raw_key);
                continue;
            }
            let (key, key_unresolved) = env_replace_lossy::<Sys>(raw_key);
            if !opts.expand_request_destination_env
                && has_env_placeholder(raw_key)
                && is_request_destination_key(&key)
            {
                auth.warn_ignored_request_destination_env(raw_key);
                continue;
            }
            if !opts.expand_auth_value_env
                && has_env_placeholder(raw_key)
                && is_auth_value_key(&key)
            {
                auth.warn_ignored_auth_value_env(raw_key);
                continue;
            }
            if !opts.expand_request_destination_env
                && has_env_placeholder(raw_value)
                && is_request_destination_value_key(&key)
            {
                auth.warn_ignored_request_destination_env(&key);
                continue;
            }
            if !opts.expand_auth_value_env
                && has_env_placeholder(raw_value)
                && is_auth_value_key(&key)
            {
                auth.warn_ignored_auth_value_env(&key);
                continue;
            }
            let (value, value_unresolved) = env_replace_lossy::<Sys>(raw_value);
            for placeholder in key_unresolved.into_iter().chain(value_unresolved) {
                auth.warnings.push(format!("Failed to replace env in config: {placeholder}"));
            }

            if key == "registry" {
                auth.registry = Some(value);
                continue;
            }
            if let Some(scope) = scoped_registry_key(&key) {
                auth.scoped_registries.insert(scope.to_string(), normalize_registry_url(&value));
                continue;
            }

            match key.as_str() {
                "https-proxy" => {
                    auth.https_proxy = Some(value);
                    continue;
                }
                "http-proxy" => {
                    auth.http_proxy = Some(value);
                    continue;
                }
                "proxy" => {
                    auth.legacy_proxy = Some(value);
                    continue;
                }
                "no-proxy" | "noproxy" => {
                    auth.no_proxy = Some(value);
                    continue;
                }
                "ca" => {
                    // Repeated `ca=` lines accumulate — matches
                    // upstream's `[null, String, Array]` nopt type
                    // where multiple values arrive as an array.
                    auth.ca.push(value);
                    continue;
                }
                "cafile" => {
                    auth.cafile = Some(resolve_cafile(value, npmrc_dir));
                    continue;
                }
                "cert" => {
                    auth.cert = Some(value);
                    continue;
                }
                "key" => {
                    auth.key = Some(value);
                    continue;
                }
                "strict-ssl" => {
                    auth.strict_ssl = parse_bool(&value);
                    continue;
                }
                "local-address" => {
                    auth.local_address = Some(value);
                    continue;
                }
                _ => {}
            }

            if let Some((uri, suffix)) = split_creds_key(&key) {
                let entry = auth.creds_entry_mut(uri);
                apply_creds_field(entry, suffix, value);
                continue;
            }

            if let Some((uri, field, is_file)) = split_ssl_key(&key) {
                // For `*file` variants the value is a path; read the
                // file at parse time (silent on error, matching
                // pnpm's `fs.readFileSync` which throws into the
                // outer parse and is swallowed). For inline variants
                // expand `\n` → real newlines so a single-line INI
                // value can carry a multi-line PEM.
                let resolved = if is_file {
                    let Ok(contents) = std::fs::read_to_string(&value) else {
                        continue;
                    };
                    contents
                } else {
                    value.replace(r"\n", "\n")
                };
                let entry = auth.tls_by_uri.entry(uri.to_owned()).or_default();
                apply_tls_field(entry, field, resolved);
                continue;
            }

            apply_creds_field(&mut auth.default_creds, key.as_str(), value);
        }
        auth
    }

    fn warn_ignored_request_destination_env(&mut self, key: &str) {
        self.warnings.push(format!(
            "Ignored project-level request destination {key:?}: environment variables are not expanded in repository-controlled registry or proxy URLs.",
        ));
    }

    fn warn_ignored_auth_value_env(&mut self, key: &str) {
        self.warnings.push(format!(
            "Ignored project-level auth setting {key:?}: environment variables are not expanded in repository-controlled registry credentials.",
        ));
    }

    /// Resolve the TLS + `local-address` slots on `config.tls`.
    ///
    /// The transformations:
    /// - Inline `ca=` PEMs are kept verbatim.
    /// - `cafile=<path>` is read from disk and split on
    ///   `-----END CERTIFICATE-----` (mirroring pnpm's
    ///   [`loadCAFile`](https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/loadNpmrcFiles.ts#L249-L265)).
    ///   Inline `ca` entries appear in the final list before the
    ///   `cafile` ones — same ordering as a `ca=` line followed by a
    ///   `cafile=` line. Unreadable `cafile` is silently dropped
    ///   (matches upstream's `catch {}` swallow).
    /// - `local-address` is parsed as [`std::net::IpAddr`]. An invalid
    ///   value is silently dropped (mirrors pnpm — Node errors at
    ///   connect time rather than load time).
    ///
    /// `strict_ssl`, `cert`, `key` are pass-through (no transformation).
    ///
    /// `cafile` paths arrive here already absolute — relative values
    /// were resolved against the `.npmrc`'s directory in
    /// [`NpmrcAuth::from_ini`] (pnpm/pnpm#11726).
    pub fn apply_tls_and_local_address(&mut self, config: &mut Config) {
        // Inline CA first, then file-loaded CA, so a user that
        // duplicates a cert across both ends up with it added twice
        // — same ordering pnpm produces.
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

    /// Resolve the `(https_proxy, http_proxy, no_proxy)` triple on
    /// `config.proxy`, mirroring upstream's
    /// [`config/reader/src/index.ts:591-600`](https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/index.ts#L591-L600)
    /// cascade. `.npmrc` always wins over env vars; the legacy `proxy=`
    /// key feeds the `httpsProxy` slot only (the http side falls back
    /// to the resolved `httpsProxy` before consulting env). `noProxy`
    /// accepts the literal token `true` to mean "bypass every proxy"
    /// — matching the `string | true` shape of upstream's
    /// [`Config.noProxy`](https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/Config.ts#L142-L146).
    ///
    /// Generic over [`EnvVar`] so cascade tests can drive every branch
    /// without mutating the process environment (no `EnvGuard` global
    /// lock).
    pub fn apply_proxy_cascade<Sys: EnvVar>(&mut self, config: &mut Config) {
        // Upstream's `getProcessEnv` tries literal-, upper-, and
        // lower-case in order (config/reader/src/index.ts:689-693). For
        // the proxy var names below the literal form is already either
        // fully upper or fully lower, so the triple collapses to two
        // real attempts.
        fn env_pair<Sys: EnvVar>(upper: &str, lower: &str) -> Option<String> {
            Sys::var(upper).or_else(|| Sys::var(lower))
        }

        config.proxy.https_proxy = self
            .https_proxy
            .take()
            .or_else(|| self.legacy_proxy.clone())
            .or_else(|| env_pair::<Sys>("HTTPS_PROXY", "https_proxy"));
        config.proxy.http_proxy = self
            .http_proxy
            .take()
            .or_else(|| config.proxy.https_proxy.clone())
            .or_else(|| env_pair::<Sys>("HTTP_PROXY", "http_proxy"))
            .or_else(|| env_pair::<Sys>("PROXY", "proxy"));
        config.proxy.no_proxy = self
            .no_proxy
            .take()
            .or_else(|| env_pair::<Sys>("NO_PROXY", "no_proxy"))
            .map(|raw| parse_no_proxy(&raw));
    }

    /// Phase 1: write the resolved `registry` onto `config` and emit
    /// any `${VAR}`-substitution warnings. Does *not* build
    /// `auth_headers` yet. Call [`NpmrcAuth::build_auth_headers`]
    /// after every other config layer (notably `pnpm-workspace.yaml`)
    /// has had a chance to override `registry`, so default-registry
    /// creds end up keyed at the final URL.
    pub fn apply_registry_and_warn(&mut self, config: &mut Config) {
        if let Some(registry) = self.registry.take() {
            config.registry =
                if registry.ends_with('/') { registry } else { format!("{registry}/") };
        }
        config.registries.append(&mut self.scoped_registries);
        for message in std::mem::take(&mut self.warnings) {
            tracing::warn!(target: "pacquet::npmrc", "{message}");
        }
    }

    /// Phase 2: compute and store the final [`AuthHeaders`] map,
    /// keying default-registry creds at `config.registry`'s nerf-darted
    /// URI. Mirrors pnpm's
    /// [`getAuthHeadersFromCreds`](https://github.com/pnpm/pnpm/blob/601317e7a3/network/auth-header/src/getAuthHeadersFromConfig.ts).
    pub fn build_auth_headers(self, config: &mut Config) {
        let mut auth_header_by_uri: HashMap<String, String> = HashMap::new();
        let mut auth_header_by_scope_by_uri: HashMap<String, HashMap<String, String>> =
            HashMap::new();
        for (uri, raw_by_scope) in self.creds_by_scope_by_uri {
            for (scope, raw) in raw_by_scope {
                if let Some(header) = creds_to_header(&raw) {
                    if scope == DEFAULT_REGISTRY_SCOPE {
                        auth_header_by_uri.insert(uri.clone(), header);
                    } else {
                        auth_header_by_scope_by_uri
                            .entry(uri.clone())
                            .or_default()
                            .insert(scope, header);
                    }
                }
            }
        }
        if !self.default_creds.is_empty()
            && let Some(header) = creds_to_header(&self.default_creds)
        {
            auth_header_by_uri.insert(pacquet_network::nerf_dart(&config.registry), header);
        }

        config.auth_headers =
            Arc::new(AuthHeaders::from_parts(auth_header_by_uri, auth_header_by_scope_by_uri));
    }

    /// Pin this source file's **unscoped** credentials (`_authToken`,
    /// `_auth`, `username`/`_password`) and client `cert`/`key` to the
    /// registry declared in this same file — or the npmjs default
    /// ([`DEFAULT_REGISTRY`]) when the file has no `registry=` of its
    /// own — by nerf-darting that registry into a per-URI key and moving
    /// the values onto [`Self::creds_by_scope_by_uri`] / [`Self::tls_by_uri`].
    ///
    /// This is the security boundary ported from pnpm's
    /// [`rescopeUnscopedCreds`](https://github.com/pnpm/pnpm/blob/1819226b51/config/reader/src/loadNpmrcFiles.ts):
    /// rescoping happens per file *before* sources are merged, so a
    /// credential can never be pulled to a different registry that a
    /// higher-priority `.npmrc` (or `pnpm-workspace.yaml`) later sets.
    /// An explicitly URL-scoped value already present for the same key
    /// is left untouched. A deprecation warning is queued (drained by
    /// [`Self::apply_registry_and_warn`]) for each rescoped field.
    ///
    /// `source_label` names the file for that warning.
    pub fn rescope_unscoped(&mut self, source_label: &str) {
        let has_creds = !self.default_creds.is_empty();
        let has_identity = self.cert.is_some() || self.key.is_some();
        if !has_creds && !has_identity {
            return;
        }

        let registry = self
            .registry
            .as_deref()
            .filter(|registry| !registry.is_empty())
            .map_or_else(|| DEFAULT_REGISTRY.to_string(), normalize_registry_url);
        let key = pacquet_network::nerf_dart(&registry);
        if key.is_empty() {
            // Unparsable registry (e.g. an unresolved `${VAR}`). Drop
            // the unscoped material rather than risk sending it to the
            // wrong host — matching pnpm's catch branch.
            self.default_creds = RawCreds::default();
            self.cert = None;
            self.key = None;
            self.warnings.push(format!(
                "Unscoped per-registry settings in {source_label:?} were ignored because the registry URL could not be parsed.",
            ));
            return;
        }

        let mut rescoped: Vec<&str> = Vec::new();
        if has_creds {
            let taken = std::mem::take(&mut self.default_creds);
            if taken.auth_token.is_some() {
                rescoped.push("_authToken");
            }
            if taken.auth_pair_base64.is_some() {
                rescoped.push("_auth");
            }
            if taken.username.is_some() {
                rescoped.push("username");
            }
            if taken.password.is_some() {
                rescoped.push("_password");
            }
            // An explicitly URL-scoped value for the same key wins, so
            // the rescoped unscoped value only fills the gaps.
            self.creds_by_scope_by_uri
                .entry(key.clone())
                .or_default()
                .entry(DEFAULT_REGISTRY_SCOPE.to_owned())
                .or_default()
                .fill_from(taken);
        }
        if has_identity {
            let entry = self.tls_by_uri.entry(key.clone()).or_default();
            if let Some(cert) = self.cert.take() {
                entry.cert.get_or_insert(cert);
                rescoped.push("cert");
            }
            if let Some(private_key) = self.key.take() {
                entry.key.get_or_insert(private_key);
                rescoped.push("key");
            }
        }
        if !rescoped.is_empty() {
            self.warnings.push(format!(
                "Unscoped per-registry settings ({}) in {source_label:?} are deprecated. \
                 pnpm pinned them to {key:?} for this run; write them as \
                 \"{key}:<setting>=...\" instead.",
                rescoped.join(", "),
            ));
        }
    }

    /// Merge a lower-priority source under `self` (the higher-priority
    /// one). Fields already set on `self` win; `lower` fills the gaps.
    /// Per-URI credential and TLS maps merge field-by-field with the
    /// same "higher wins" rule. Mirrors pnpm's merge order
    /// (`user < auth.ini < workspace`), where each later source
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
        self.apply_registry_and_warn(config);
        self.apply_proxy_cascade::<Sys>(config);
        self.apply_tls_and_local_address(config);
        self.build_auth_headers(config);
    }

    fn creds_entry_mut(&mut self, uri: &str) -> &mut RawCreds {
        let (registry_uri, scope) = split_scope_from_uri(uri);
        self.creds_by_scope_by_uri
            .entry(registry_uri)
            .or_default()
            .entry(scope.unwrap_or_else(|| DEFAULT_REGISTRY_SCOPE.to_owned()))
            .or_default()
    }
}

/// Normalize a registry URL the way pnpm's `normalizeRegistryUrl` does
/// for the purposes of nerf-darting: ensure a single trailing slash.
fn normalize_registry_url(registry: &str) -> String {
    if registry.ends_with('/') { registry.to_string() } else { format!("{registry}/") }
}

/// Resolve a top-level `cafile=` value against the directory of the
/// `.npmrc` that declared it. Empty and absolute values pass through
/// unchanged; relative values are joined onto `npmrc_dir`. Mirrors
/// pnpm/pnpm#11726.
fn resolve_cafile(value: String, npmrc_dir: &Path) -> String {
    if value.is_empty() || Path::new(&value).is_absolute() {
        return value;
    }
    let resolved: PathBuf = npmrc_dir.join(&value);
    resolved.into_os_string().into_string().unwrap_or(value)
}

/// Parse a `strict-ssl=…` value. pnpm/nopt accepts only the literal
/// `true` and `false` tokens; anything else is dropped silently so the
/// dispatcher's per-emit `strictSsl ?? true` default kicks in.
fn parse_bool(value: &str) -> Option<bool> {
    match value.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn is_request_destination_key(key: &str) -> bool {
    is_registry_key(key) || key.starts_with("//")
}

fn is_request_destination_value_key(key: &str) -> bool {
    is_registry_key(key) || matches!(key, "https-proxy" | "http-proxy" | "proxy")
}

fn is_registry_key(key: &str) -> bool {
    key == "registry" || (key.starts_with('@') && key.ends_with(":registry"))
}

fn scoped_registry_key(key: &str) -> Option<&str> {
    key.strip_suffix(":registry")
        .filter(|scope| scope.starts_with('@') && scope.len() > 1 && !scope.contains('/'))
}

fn has_env_placeholder(value: &str) -> bool {
    value
        .match_indices("${")
        .any(|(start, _)| value[start + 2..].find('}').is_some_and(|end| end > 0))
}

/// Read a `cafile` path and split the contents on
/// `-----END CERTIFICATE-----` to produce one PEM per certificate.
/// Mirrors pnpm's
/// [`loadCAFile`](https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/loadNpmrcFiles.ts#L238-L265):
/// re-append the delimiter to each split, trim, drop empties, and
/// silently treat any read error as an empty list.
fn load_cafile(path: &Path) -> Vec<String> {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let delimiter = "-----END CERTIFICATE-----";
    // Byte-for-byte parity with pnpm's
    // [`loadCAFile`](https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/loadNpmrcFiles.ts#L251-L254):
    // `contents.split(delim).filter(ca => ca.trim().length > 0).map(ca => `${ca.trimStart()}${delim}`)`.
    //
    // Key contract points:
    // - `split` (not `split_inclusive`) — pnpm drops the delimiter
    //   from each chunk and re-appends it on the map side.
    // - Filter on `chunk.trim().is_empty()` — drops the trailing
    //   empty chunk produced when the file ends with a delimiter,
    //   but *keeps* a trailing non-empty (malformed) chunk so
    //   downstream `Certificate::from_pem` surfaces the parse error
    //   instead of pacquet silently dropping the entry.
    // - `trim_start()` (not full `trim`) — pnpm preserves any
    //   trailing whitespace inside the chunk before the appended
    //   delimiter. Doesn't matter to a PEM parser but matters for
    //   "is the output byte-equivalent to pnpm's" tests.
    contents
        .split(delimiter)
        .filter(|chunk| !chunk.trim().is_empty())
        .map(|chunk| format!("{}{}", chunk.trim_start(), delimiter))
        .collect()
}

/// Parse the raw `no-proxy` value into [`NoProxySetting`].
///
/// `"true"` (after trimming) is the literal-`true` shape from upstream's
/// `noProxy: string | true` type. Anything else is comma-split, trimmed,
/// empties dropped.
fn parse_no_proxy(raw: &str) -> NoProxySetting {
    if raw.trim() == "true" {
        return NoProxySetting::Bypass;
    }
    NoProxySetting::List(
        raw.split(',').map(str::trim).filter(|item| !item.is_empty()).map(String::from).collect(),
    )
}

/// Convert raw .npmrc credentials into the `Authorization` header
/// value pnpm would send. Returns `None` if no usable credential
/// shape is present.
fn creds_to_header(creds: &RawCreds) -> Option<String> {
    if let Some(token) = &creds.auth_token {
        return Some(format!("Bearer {token}"));
    }
    if let Some(pair) = &creds.auth_pair_base64 {
        return Some(format!("Basic {pair}"));
    }
    if let (Some(user), Some(pass_b64)) = (&creds.username, &creds.password) {
        // npm encodes `_password` as base64 of the raw password. The
        // header itself is `Basic base64(user:password)`, so we decode
        // the password back and re-encode the pair, matching pnpm's
        // [`parseBasicAuth`](https://github.com/pnpm/pnpm/blob/601317e7a3/config/reader/src/parseCreds.ts#L56-L77).
        let password = base64_decode(pass_b64).unwrap_or_else(|| pass_b64.clone());
        return Some(format!("Basic {}", base64_encode(&format!("{user}:{password}"))));
    }
    None
}

/// Decode a standard base64 string. Used for the `_password` field
/// where npm stores the raw password base64-encoded; falls back to
/// returning `None` so the caller can keep the raw value verbatim
/// when the input is not valid base64.
fn base64_decode(input: &str) -> Option<String> {
    let cleaned: Vec<u8> = input.bytes().filter(|byte| !byte.is_ascii_whitespace()).collect();
    let mut bytes = Vec::with_capacity(cleaned.len() / 4 * 3);
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for byte in cleaned {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => return None,
        };
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    String::from_utf8(bytes).ok()
}

/// Auth-suffix keys recognised on a `//host[:port]/path/:` prefix,
/// mirroring `AUTH_SUFFIX_RE` from pnpm's `getNetworkConfigs.ts`.
const CREDS_SUFFIXES: &[&str] = &["_authToken", "_auth", "_password", "username"];

fn is_auth_value_key(key: &str) -> bool {
    matches!(key, "_authToken" | "_auth" | "_password" | "username" | "cert" | "key")
        || split_creds_key(key).is_some()
        || split_inline_identity_key(key).is_some()
}

/// Match a `npm_config_//…` / `pnpm_config_//…` environment variable name.
/// Returns `(is_pnpm, key)` where `key` is the URL-scoped remainder
/// (e.g. `//registry.npmjs.org/:_authToken`) with its case preserved.
/// The prefix is matched case-insensitively, mirroring npm. `None` for any
/// name that isn't one of these prefixes followed by `//`.
fn parse_url_scoped_env_name(name: &str) -> Option<(bool, &str)> {
    for (prefix, is_pnpm) in [("pnpm_config_", true), ("npm_config_", false)] {
        // Slice with `get` rather than `name[..prefix.len()]`: a byte index
        // landing inside a multi-byte char (a non-ASCII env var name) would
        // otherwise panic before the prefix check can reject it.
        let (Some(head), Some(rest)) = (name.get(..prefix.len()), name.get(prefix.len()..)) else {
            continue;
        };
        if head.eq_ignore_ascii_case(prefix) && rest.starts_with("//") {
            return Some((is_pnpm, rest));
        }
    }
    None
}

fn split_creds_key(key: &str) -> Option<(&str, &str)> {
    if !key.starts_with("//") {
        return None;
    }
    for suffix in CREDS_SUFFIXES {
        let needle = format!(":{suffix}");
        if let Some(stripped) = key.strip_suffix(needle.as_str()) {
            return Some((stripped, suffix));
        }
    }
    None
}

fn split_scope_from_uri(uri: &str) -> (String, Option<String>) {
    if let Some((registry_uri, scope)) = split_scope_from_uri_by_colon(uri) {
        return (normalize_registry_key(registry_uri), Some(scope.to_owned()));
    }
    split_scope_from_uri_by_path(uri)
}

fn split_scope_from_uri_by_colon(uri: &str) -> Option<(&str, &str)> {
    if !uri.starts_with("//") {
        return None;
    }
    let scope_separator_index = uri.rfind(":@")?;
    let scope = &uri[scope_separator_index + 1..];
    if !is_package_scope(scope) {
        return None;
    }
    Some((&uri[..scope_separator_index], scope))
}

fn split_scope_from_uri_by_path(uri: &str) -> (String, Option<String>) {
    let trimmed = uri.strip_suffix('/').unwrap_or(uri);
    let Some(last_slash_index) = trimmed.rfind('/') else {
        return (uri.to_owned(), None);
    };
    let scope = &trimmed[last_slash_index + 1..];
    if !is_package_scope(scope) {
        return (uri.to_owned(), None);
    }
    (trimmed[..=last_slash_index].to_owned(), Some(scope.to_owned()))
}

fn is_package_scope(scope: &str) -> bool {
    scope.starts_with('@') && scope.len() > 1 && !scope.contains('/') && !scope.contains(':')
}

fn normalize_registry_key(registry: &str) -> String {
    if registry.ends_with('/') { registry.to_owned() } else { format!("{registry}/") }
}

fn apply_creds_field(creds: &mut RawCreds, field: &str, value: String) {
    // The catch-all swallows arbitrary `.npmrc` keys that don't map to
    // a credential field. Examples: a top-level `store-dir=` line, or
    // a `//host/:registry=` per-registry override that we don't honour
    // yet. Matches pnpm's `getNetworkConfigs` shape: only the four
    // recognised fields contribute to `RawCreds`; everything else is
    // silently dropped.
    match field {
        "_authToken" => creds.auth_token = Some(value),
        "_auth" => creds.auth_pair_base64 = Some(value),
        "username" => creds.username = Some(value),
        "_password" => creds.password = Some(value),
        _ => {}
    }
}

/// Per-registry TLS suffixes. The `*file` variants instruct the
/// parser to read the value as a path; the bare variants use the
/// value as inline PEM (with `\n` escape expansion). Mirrors
/// `SSL_SUFFIX_RE = /:(?<id>cert|key|ca)(?<kind>file)?$/` from pnpm's
/// [`getNetworkConfigs.ts:94`](https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/getNetworkConfigs.ts#L94).
const TLS_SUFFIXES: &[(&str, &str, bool)] = &[
    // (suffix, field, is_file)
    (":cafile", "ca", true),
    (":certfile", "cert", true),
    (":keyfile", "key", true),
    (":ca", "ca", false),
    (":cert", "cert", false),
    (":key", "key", false),
];

/// Return `(uri_prefix, field, is_file)` when `key` ends in one of the
/// recognized TLS suffixes. Matches pnpm's `tryParseSslKey` —
/// deliberately does *not* require a leading `//`, so the lax keys
/// pnpm accepts (`foo:cert=…`) end up in the map with `uri_prefix =
/// "foo"`. They never match a real nerf-darted URL so the entry is
/// effectively dropped at lookup time, but storing it preserves
/// byte-for-byte parity with upstream parsing.
///
/// Order matters: `:certfile` must be tested before `:cert` so the
/// `*file` variants don't get parsed as the inline form with a
/// trailing `file` artifact in the URI prefix.
fn split_ssl_key(key: &str) -> Option<(&str, &'static str, bool)> {
    for (suffix, field, is_file) in TLS_SUFFIXES {
        if let Some(stripped) = key.strip_suffix(suffix) {
            return Some((stripped, field, *is_file));
        }
    }
    None
}

fn split_inline_identity_key(key: &str) -> Option<(&str, &'static str)> {
    let (uri, field, is_file) = split_ssl_key(key)?;
    (!is_file && matches!(field, "cert" | "key")).then_some((uri, field))
}

/// Write a per-registry TLS value onto a [`RegistryTls`] entry.
///
/// For inline values (`is_file = false`) the parser pre-expands `\n`
/// escapes to real newlines — pnpm does this only on per-registry
/// values, not on the top-level `ca=` form
/// ([`getNetworkConfigs.ts:38-39`](https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/getNetworkConfigs.ts#L38-L39))
/// — and `value` arrives already expanded.
fn apply_tls_field(tls: &mut RegistryTls, field: &str, value: String) {
    match field {
        "ca" => tls.ca = Some(value),
        "cert" => tls.cert = Some(value),
        "key" => tls.key = Some(value),
        _ => {}
    }
}

#[cfg(test)]
mod tests;
