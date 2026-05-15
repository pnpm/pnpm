use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use pacquet_network::{AuthHeaders, NoProxySetting, PerRegistryTls, RegistryTls, base64_encode};

use crate::{Config, api::EnvVar, env_replace::env_replace_lossy};

/// Subset of `.npmrc` keys pacquet honours for registry / auth setup.
///
/// The parser pulls out:
/// * the top-level `registry=` URL (already supported pre-#336),
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
/// Other `.npmrc` knobs (scoped `@scope:registry`, per-registry TLS
/// like `//host:cafile=`, etc.) remain unparsed for now. See the
/// upstream
/// [`isIniConfigKey`](https://github.com/pnpm/pnpm/blob/601317e7a3/config/reader/src/localConfig.ts#L160-L161)
/// list. They will land here as the matching feature work picks them
/// up.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct NpmrcAuth {
    pub registry: Option<String>,
    /// Default-registry creds (i.e. `_auth=…`, `_authToken=…`,
    /// `username=…` / `_password=…` without a leading `//host/`).
    /// Applied to whichever URI the resolved `registry` points at.
    pub default_creds: RawCreds,
    /// Per-URI creds, keyed by the literal `.npmrc` key prefix
    /// (`//host[:port]/path/`). The map is preserved verbatim through
    /// to [`AuthHeaders`] construction so the lookup keys stay
    /// byte-equivalent to upstream.
    pub creds_by_uri: HashMap<String, RawCreds>,
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
    /// `cafile`-not-found is silently treated as unset.
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
#[derive(Debug, Default, PartialEq, Eq, Clone)]
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

impl RawCreds {
    fn is_empty(&self) -> bool {
        self.auth_token.is_none()
            && self.auth_pair_base64.is_none()
            && self.username.is_none()
            && self.password.is_none()
    }
}

impl NpmrcAuth {
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
    pub fn from_ini<Api: EnvVar>(text: &str) -> Self {
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
            let (key, key_unresolved) = env_replace_lossy::<Api>(raw_key);
            let (value, value_unresolved) = env_replace_lossy::<Api>(raw_value);
            for placeholder in key_unresolved.into_iter().chain(value_unresolved) {
                auth.warnings.push(format!("Failed to replace env in config: {placeholder}"));
            }

            if key == "registry" {
                auth.registry = Some(value);
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
                    auth.cafile = Some(value);
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
                    // pnpm/nopt parses `true` / `false` case-sensitively.
                    // Anything else resets the slot to `None` so the
                    // build-site `unwrap_or(true)` default kicks in —
                    // matters when the same `.npmrc` has multiple
                    // `strict-ssl=` lines and a later invalid token
                    // would otherwise leave an earlier `false`
                    // silently active.
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
                let entry = auth.creds_by_uri.entry(uri.to_owned()).or_default();
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
                    value.replace("\\n", "\n")
                };
                let entry = auth.tls_by_uri.entry(uri.to_owned()).or_default();
                apply_tls_field(entry, field, resolved);
                continue;
            }

            apply_creds_field(&mut auth.default_creds, key.as_str(), value);
        }
        auth
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
    /// `cafile` reads relative paths against the process cwd, matching
    /// pnpm's `path.resolve(cafile)` in
    /// [`loadCAFile`](https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/loadNpmrcFiles.ts#L241-L243).
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
    pub fn apply_proxy_cascade<Api: EnvVar>(&mut self, config: &mut Config) {
        // Upstream's `getProcessEnv` tries literal-, upper-, and
        // lower-case in order (config/reader/src/index.ts:689-693). For
        // the proxy var names below the literal form is already either
        // fully upper or fully lower, so the triple collapses to two
        // real attempts.
        fn env_pair<Api: EnvVar>(upper: &str, lower: &str) -> Option<String> {
            Api::var(upper).or_else(|| Api::var(lower))
        }

        config.proxy.https_proxy = self
            .https_proxy
            .take()
            .or_else(|| self.legacy_proxy.clone())
            .or_else(|| env_pair::<Api>("HTTPS_PROXY", "https_proxy"));
        config.proxy.http_proxy = self
            .http_proxy
            .take()
            .or_else(|| config.proxy.https_proxy.clone())
            .or_else(|| env_pair::<Api>("HTTP_PROXY", "http_proxy"))
            .or_else(|| env_pair::<Api>("PROXY", "proxy"));
        config.proxy.no_proxy = self
            .no_proxy
            .take()
            .or_else(|| env_pair::<Api>("NO_PROXY", "no_proxy"))
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
        for (uri, raw) in self.creds_by_uri {
            if let Some(header) = creds_to_header(&raw) {
                auth_header_by_uri.insert(uri, header);
            }
        }
        // Default-registry creds are passed through with an empty-string
        // key, matching upstream's
        // [`getAuthHeadersFromCreds`](https://github.com/pnpm/pnpm/blob/601317e7a3/network/auth-header/src/getAuthHeadersFromConfig.ts)
        // where `configByUri['']` holds default creds and is re-keyed
        // onto `defaultRegistry` by the constructor.
        // [`AuthHeaders::from_creds_map`] does the nerf-darting.
        if !self.default_creds.is_empty()
            && let Some(header) = creds_to_header(&self.default_creds)
        {
            auth_header_by_uri.insert(String::new(), header);
        }

        config.auth_headers =
            Arc::new(AuthHeaders::from_creds_map(auth_header_by_uri, Some(&config.registry)));
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
    pub fn apply_to<Api: EnvVar>(mut self, config: &mut Config) {
        self.apply_registry_and_warn(config);
        self.apply_proxy_cascade::<Api>(config);
        self.apply_tls_and_local_address(config);
        self.build_auth_headers(config);
    }
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

/// Read a `cafile` path and split the contents on
/// `-----END CERTIFICATE-----` to produce one PEM per certificate.
/// Mirrors pnpm's
/// [`loadCAFile`](https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/loadNpmrcFiles.ts#L238-L265):
/// re-append the delimiter to each split, trim, drop empties, and
/// silently treat any read error as an empty list.
///
/// **Relative-path resolution caveat.** Pnpm's `loadCAFile` passes
/// `cafile` straight to `fs.readFileSync` without `path.resolve` —
/// relative paths therefore resolve against Node's `process.cwd()`,
/// *not* against the directory the `.npmrc` was read from. Pnpm
/// doesn't `process.chdir(opts.dir)` on `--dir`, so a project
/// `.npmrc` containing `cafile=certs/ca.pem` invoked as
/// `pnpm --dir /project install` from `/home/user` reads
/// `/home/user/certs/ca.pem` and silently drops the CA list when it
/// isn't found. Pacquet matches that exact behavior (cardinal rule:
/// match pnpm even when the upstream behavior is surprising). Users
/// who hit this should either use an absolute path in `cafile=`,
/// `cd` into the project directory before running pacquet, or set
/// the `ca=` inline form which doesn't read from disk.
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
        raw.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect(),
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
