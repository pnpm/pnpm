use super::{
    Arc, AuthHeaders, BTreeMap, Config, DEFAULT_REGISTRY, DEFAULT_REGISTRY_SCOPE, HashMap,
    LoadWorkspaceYamlError, NpmrcAuth, base64_encode, base64_encode_bytes, normalize_registry_url,
    parse_token_helper_field, split_scope_from_uri,
};

/// Raw (unparsed) credential fields for a given registry URI.
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
    /// `tokenHelper=` value: the raw command line naming an executable
    /// pnpm runs to obtain the registry token. Kept as the raw string
    /// here; validated (reserved characters) and split into a command
    /// at [`NpmrcAuth::build_auth_headers`] time. Only honored from a
    /// trusted, non-repo source (see the trust guard in
    /// [`crate::Config::current`]); a project/workspace `.npmrc`
    /// carrying one is rejected.
    pub token_helper: Option<String>,
}

impl RawCreds {
    fn is_empty(&self) -> bool {
        self.auth_token.is_none()
            && self.auth_pair_base64.is_none()
            && self.username.is_none()
            && self.password.is_none()
            && self.token_helper.is_none()
    }

    /// Fill any field that is `None` here from `lower`. Used when
    /// merging a lower-priority source under a higher one: the higher
    /// source's already-set fields win, the lower fills the gaps.
    pub(super) fn fill_from(&mut self, lower: RawCreds) {
        self.auth_token = self.auth_token.take().or(lower.auth_token);
        self.auth_pair_base64 = self.auth_pair_base64.take().or(lower.auth_pair_base64);
        self.username = self.username.take().or(lower.username);
        self.password = self.password.take().or(lower.password);
        self.token_helper = self.token_helper.take().or(lower.token_helper);
    }
}

/// The per-registry lookups [`NpmrcAuth::build_auth_headers`] fills, one
/// credential at a time.
#[derive(Default)]
struct AuthTables {
    auth_headers: HashMap<String, String>,
    scoped_auth_headers: HashMap<String, HashMap<String, String>>,
    token_helpers: HashMap<String, Vec<String>>,
    scoped_token_helpers: HashMap<String, HashMap<String, Vec<String>>>,
    /// See [`Config::auth_tokens_by_uri`].
    auth_tokens: HashMap<String, String>,
    /// See [`Config::registry_creds_by_uri`].
    registry_creds: HashMap<String, BTreeMap<String, RegistryCreds>>,
}

impl AuthTables {
    /// Record the credential `raw` names for `scope` at `uri`.
    fn record(
        &mut self,
        uri: &str,
        scope: String,
        raw: &RawCreds,
    ) -> Result<(), LoadWorkspaceYamlError> {
        if let Some(token) = default_scope_token(&scope, raw) {
            self.auth_tokens.insert(uri.to_owned(), token);
        }
        let creds = RegistryCreds::from_raw(raw)?;
        if let Some(command) = creds.token_helper.clone() {
            insert_by_scope(
                &mut self.token_helpers,
                &mut self.scoped_token_helpers,
                uri,
                scope.clone(),
                command,
            );
        } else if let Some(header) = creds_to_header(raw)? {
            insert_by_scope(
                &mut self.auth_headers,
                &mut self.scoped_auth_headers,
                uri,
                scope.clone(),
                header,
            );
        }
        if creds != RegistryCreds::default() {
            self.registry_creds.entry(uri.to_owned()).or_default().insert(scope, creds);
        }
        Ok(())
    }
}

/// A registry's credentials for one scope, in the shape pnpm reports them
/// under `configByUri`.
///
/// The token is reported as written, the basic-auth pair decoded, and the
/// `tokenHelper` split into its command. An empty token or an empty half of
/// a pair, the shape an unresolved `${VAR}` leaves, names no credential, as
/// on pnpm 11. A `_auth` that does not decode is left out of this view;
/// whether it fails the load is decided where the `Authorization` header
/// is built.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryCreds {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub basic_auth: Option<BasicAuth>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_helper: Option<Vec<String>>,
}

/// A decoded `username` / `password` pair of a [`RegistryCreds`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct BasicAuth {
    pub username: String,
    pub password: String,
}

impl RegistryCreds {
    /// Fails only where the `tokenHelper` value is not a command pnpm runs;
    /// see [`parse_token_helper_field`].
    fn from_raw(raw: &RawCreds) -> Result<Self, LoadWorkspaceYamlError> {
        Ok(Self {
            auth_token: raw.auth_token.clone().filter(|token| !token.is_empty()),
            basic_auth: decode_basic_auth(raw),
            token_helper: parse_token_helper_field(raw.token_helper.as_deref())?,
        })
    }
}

/// The `username:password` pair `raw` carries, from a base64 `_auth` or
/// from `username` plus a base64 `_password`, read the way
/// [`creds_to_header`] reads them: a `_password` that is not base64 is the
/// password as written, since that is what the header carries. `None` when
/// `raw` names no complete pair or `_auth` does not decode.
fn decode_basic_auth(raw: &RawCreds) -> Option<BasicAuth> {
    if let Some(pair) = raw.auth_pair_base64.as_deref().filter(|pair| !pair.is_empty()) {
        let decoded = base64_decode(pair)?;
        let (username, password) = decoded.split_once(':')?;
        return Some(BasicAuth { username: username.to_owned(), password: password.to_owned() });
    }
    let username = raw.username.clone().filter(|username| !username.is_empty())?;
    let password_b64 = raw.password.as_ref().filter(|password| !password.is_empty())?;
    let password = base64_decode(password_b64).unwrap_or_else(|| password_b64.clone());
    Some(BasicAuth { username, password })
}

/// The registry a file's unscoped settings pin to: the one it declares, or
/// the npmjs default when it declares none.
fn pinned_registry(declared: &str) -> String {
    if declared.is_empty() {
        return DEFAULT_REGISTRY.to_owned();
    }
    normalize_registry_url(declared)
}

/// The raw `_authToken` of a default-scope credential, kept alongside the
/// baked header for `pnpm logout`.
fn default_scope_token(scope: &str, raw: &RawCreds) -> Option<String> {
    (scope == DEFAULT_REGISTRY_SCOPE).then(|| raw.auth_token.clone()).flatten()
}

/// Record a per-registry value in the default-scope map or in the
/// scoped one, whichever the scope names.
fn insert_by_scope<Value>(
    by_uri: &mut HashMap<String, Value>,
    by_scope_by_uri: &mut HashMap<String, HashMap<String, Value>>,
    uri: &str,
    scope: String,
    value: Value,
) {
    if scope == DEFAULT_REGISTRY_SCOPE {
        by_uri.insert(uri.to_owned(), value);
    } else {
        by_scope_by_uri.entry(uri.to_owned()).or_default().insert(scope, value);
    }
}

/// The unscoped per-registry settings [`NpmrcAuth::rescope_unscoped`]
/// pins, in the order pnpm reports them. `ca` / `cafile` are deliberately
/// absent: they are trust anchors rather than credentials, and corporate
/// MITM-proxy setups rely on them applying to every HTTPS request, so a
/// default-registry override cannot weaponize them.
fn unscoped_key_names(creds: &RawCreds, has_cert: bool, has_key: bool) -> Vec<&'static str> {
    [
        ("_authToken", creds.auth_token.is_some()),
        ("_auth", creds.auth_pair_base64.is_some()),
        ("username", creds.username.is_some()),
        ("_password", creds.password.is_some()),
        ("tokenHelper", creds.token_helper.is_some()),
        ("cert", has_cert),
        ("key", has_key),
    ]
    .into_iter()
    .filter_map(|(name, is_set)| is_set.then_some(name))
    .collect()
}

/// Convert raw .npmrc credentials into an `Authorization` header
/// value. Returns `None` if no usable credential shape is present.
///
/// `_auth` is decoded and re-encoded rather than forwarded: an `.npmrc`
/// carries spellings the wire format does not (embedded whitespace,
/// redundant or missing `=` padding), and only the canonical encoding of
/// the credential it names is a header a registry can read.
fn creds_to_header(creds: &RawCreds) -> Result<Option<String>, LoadWorkspaceYamlError> {
    if let Some(token) = &creds.auth_token {
        return Ok(Some(format!("Bearer {token}")));
    }
    // An empty `_auth` names no credential — the shape an unresolved
    // `${VAR}` leaves behind — and pnpm skips it rather than failing.
    if let Some(pair) = creds.auth_pair_base64.as_deref().filter(|pair| !pair.is_empty()) {
        let decoded = base64_decode_bytes(pair)
            .ok_or(LoadWorkspaceYamlError::AuthInvalidBase64 { key: "_auth" })?;
        if !decoded.contains(&b':') {
            return Err(LoadWorkspaceYamlError::AuthMissingSeparator);
        }
        return Ok(Some(format!("Basic {}", base64_encode_bytes(&decoded))));
    }
    if let (Some(user), Some(pass_b64)) = (&creds.username, &creds.password) {
        // npm encodes `_password` as base64 of the raw password. The
        // header itself is `Basic base64(user:password)`, so we decode
        // the password back and re-encode the pair.
        let password = base64_decode(pass_b64).unwrap_or_else(|| pass_b64.clone());
        return Ok(Some(format!("Basic {}", base64_encode(&format!("{user}:{password}")))));
    }
    Ok(None)
}

/// The decoder pnpm reads credentials with: `atob`'s forgiving-base64,
/// which ignores whitespace anywhere and keeps the low bits of a
/// truncated final group rather than rejecting the value. Padding is
/// stripped before the value reaches this engine, so `=` left anywhere
/// else is an invalid byte — as it is for `atob`.
const CREDENTIAL_BASE64: base64::engine::GeneralPurpose = base64::engine::GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    base64::engine::GeneralPurposeConfig::new()
        .with_decode_allow_trailing_bits(true)
        .with_decode_padding_mode(base64::engine::DecodePaddingMode::RequireNone),
);

/// Decode a standard base64 credential to the bytes it carries, matching
/// `decodeBase64Credential` in the TypeScript stack: whitespace is
/// ignored, and trailing `=` padding may be redundant (`aGk===`),
/// short (`Zm9vOmJhcg=`), or missing (`aGk`) — the spellings that reach
/// a `.npmrc` by hand or from a shell pipeline. `None` when the value is
/// not base64 at all.
fn base64_decode_bytes(input: &str) -> Option<Vec<u8>> {
    let cleaned: Vec<u8> = input.bytes().filter(|byte| !byte.is_ascii_whitespace()).collect();
    let Some(last) = cleaned.iter().rposition(|byte| *byte != b'=') else {
        // Padding with nothing to pad is not base64; `atob` rejects it,
        // and only an empty value decodes to nothing.
        return cleaned.is_empty().then(Vec::new);
    };
    base64::Engine::decode(&CREDENTIAL_BASE64, &cleaned[..=last]).ok()
}

/// [`base64_decode_bytes`] for the `_password` field, whose decoded form
/// is read as text. `None` — so the caller can keep the raw value
/// verbatim — when the input is not base64 or not UTF-8.
pub(super) fn base64_decode(input: &str) -> Option<String> {
    String::from_utf8(base64_decode_bytes(input)?).ok()
}

pub(super) fn apply_creds_field(creds: &mut RawCreds, field: &str, value: String) {
    // The catch-all swallows arbitrary `.npmrc` keys that don't map to
    // a credential field. Examples: a top-level `store-dir=` line, or
    // a `//host/:registry=` per-registry override that we don't honour
    // yet. Only the four recognised fields contribute to `RawCreds`;
    // everything else is silently dropped.
    match field {
        "_authToken" => creds.auth_token = Some(value),
        "_auth" => creds.auth_pair_base64 = Some(value),
        "username" => creds.username = Some(value),
        "_password" => creds.password = Some(value),
        "tokenHelper" => creds.token_helper = Some(value),
        _ => {}
    }
}

impl NpmrcAuth {
    /// Phase 2: compute and store the final [`AuthHeaders`] map.
    ///
    /// Every credential is keyed at the URI it was authored for:
    /// [`Self::rescope_unscoped`] has already pinned the unscoped ones to
    /// their own source's registry, so nothing here consults
    /// `config.registry` and a later layer overriding it cannot move a
    /// credential to another host.
    ///
    /// A `tokenHelper` credential becomes an un-executed command in the
    /// [`AuthHeaders`] (run lazily on lookup) rather than a baked header;
    /// like pnpm's `credsToHeader`, a `tokenHelper` wins over any static
    /// token on the same registry. Fails with
    /// [`LoadWorkspaceYamlError::TokenHelperUnsupportedCharacter`] if a
    /// honored helper's value contains a reserved character.
    pub fn build_auth_headers(self, config: &mut Config) -> Result<(), LoadWorkspaceYamlError> {
        debug_assert!(
            self.default_creds.is_empty(),
            "rescope_unscoped must pin unscoped credentials before headers are built",
        );
        let mut tables = AuthTables::default();
        for (uri, raw_by_scope) in self.creds_by_scope_by_uri {
            for (scope, raw) in raw_by_scope {
                tables.record(&uri, scope, &raw)?;
            }
        }
        config.auth_tokens_by_uri = tables.auth_tokens;
        config.registry_creds_by_uri = tables.registry_creds;
        config.auth_headers = Arc::new(AuthHeaders::from_parts_with_token_helpers(
            tables.auth_headers,
            tables.scoped_auth_headers,
            tables.token_helpers,
            tables.scoped_token_helpers,
        ));
        Ok(())
    }

    /// Pin this source file's **unscoped** per-registry settings
    /// ([`unscoped_key_names`]) to the registry declared in this same
    /// file — or the npmjs default ([`DEFAULT_REGISTRY`]) when the file has
    /// no `registry=` of its own — by nerf-darting that registry into a
    /// per-URI key and moving the values onto
    /// [`Self::creds_by_scope_by_uri`] / [`Self::tls_by_uri`], plus the
    /// matching rewrite of [`Self::raw_ini_config`] so `pnpm config get` /
    /// `pnpm config list` report the pinned spelling.
    ///
    /// This is a security boundary: rescoping happens per file *before*
    /// sources are merged, so a credential can never be pulled to a
    /// different registry that a higher-priority `.npmrc` (or
    /// `pnpm-workspace.yaml`) later sets.
    /// An explicitly URL-scoped value already present for the same key
    /// is left untouched. A deprecation warning is queued (drained by
    /// [`Self::apply_registry_and_warn`]) for each rescoped field.
    ///
    /// `source_label` names the file for that warning.
    pub fn rescope_unscoped(&mut self, source_label: &str) {
        let creds = std::mem::take(&mut self.default_creds);
        let cert = self.cert.take();
        let private_key = self.key.take();
        let unscoped = unscoped_key_names(&creds, cert.is_some(), private_key.is_some());
        if unscoped.is_empty() {
            return;
        }
        let names = unscoped.join(", ");
        let raw_values = self.take_unscoped_raw_values(&unscoped, &creds);

        let declared_registry = self
            .registry
            .as_deref()
            .filter(|registry| !registry.is_empty())
            .unwrap_or_default()
            .to_owned();
        let uri = pnpm_network::nerf_dart(&pinned_registry(&declared_registry));
        if uri.is_empty() {
            // Unparsable registry (e.g. an unresolved `${VAR}`). Drop
            // the unscoped material — already taken above — rather than
            // risk sending it to the wrong host.
            self.warnings.push(format!(
                "Unscoped per-registry settings ({names}) in \"{source_label}\" were ignored: \
                 the source's \"registry\" value ({declared_registry:?}) is not a parseable URL, \
                 so pnpm cannot pin them anywhere safe. Write them URL-scoped \
                 (e.g. \"//registry.example.com/:_authToken=...\") to send them to a specific \
                 registry.",
            ));
            return;
        }

        self.fill_scoped_credentials(&uri, creds, cert, private_key);

        for (raw_key, value) in raw_values {
            self.raw_ini_config.entry(format!("{uri}:{raw_key}")).or_insert(value);
        }
        self.warnings.push(format!(
            "Unscoped per-registry settings ({names}) in \"{source_label}\" are deprecated. \
             pnpm pinned them to {uri:?} for this run, but a future release will stop supporting \
             unscoped per-registry settings. Write them as \"{uri}:{}=...\" instead.",
            unscoped[0],
        ));
    }

    pub(super) fn fill_scoped_credentials(
        &mut self,
        uri: &str,
        creds: RawCreds,
        cert: Option<String>,
        private_key: Option<String>,
    ) {
        // An explicitly URL-scoped value for the same key wins, so the
        // rescoped value only fills the gaps.
        if !creds.is_empty() {
            let by_scope = self.creds_by_scope_by_uri.entry(uri.to_owned()).or_default();
            by_scope.entry(DEFAULT_REGISTRY_SCOPE.to_owned()).or_default().fill_from(creds);
        }
        if cert.is_some() || private_key.is_some() {
            let entry = self.tls_by_uri.entry(uri.to_owned()).or_default();
            entry.cert = entry.cert.take().or(cert);
            entry.key = entry.key.take().or(private_key);
        }
    }

    /// Take each setting's raw INI spelling along with its structured
    /// value, so both move to the pinned key together. `tokenHelper` is
    /// not an INI-readable key on its own ([`crate::config_types::is_ini_config_key`] mirrors
    /// npm, which has no unscoped form), so the parser never captured it
    /// — its raw value comes from the parsed credential instead.
    pub(super) fn take_unscoped_raw_values(
        &mut self,
        unscoped: &[&'static str],
        creds: &RawCreds,
    ) -> Vec<(&'static str, String)> {
        unscoped
            .iter()
            .filter_map(|key| {
                self.raw_ini_config
                    .remove(*key)
                    .or_else(|| {
                        (*key == "tokenHelper").then(|| creds.token_helper.clone()).flatten()
                    })
                    .map(|value| (*key, value))
            })
            .collect()
    }

    pub(super) fn creds_entry_mut(&mut self, uri: &str) -> &mut RawCreds {
        let (registry_uri, scope) = split_scope_from_uri(uri);
        self.creds_by_scope_by_uri
            .entry(registry_uri)
            .or_default()
            .entry(scope.unwrap_or_else(|| DEFAULT_REGISTRY_SCOPE.to_owned()))
            .or_default()
    }
}
