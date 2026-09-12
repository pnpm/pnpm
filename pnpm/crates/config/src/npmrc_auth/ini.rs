use super::{
    Cow, EnvVar, NpmrcAuth, Path, apply_creds_field, apply_tls_field, env_replace_lossy,
    expand_inline_pem, is_auth_value_key, normalize_registry_url, parse_bool, resolve_cafile,
    split_ini_creds_key, split_ssl_key,
};

#[derive(Clone, Copy)]
struct ParseOptions {
    expand_auth_value_env: bool,
    expand_request_destination_env: bool,
}

/// One `key=value` line, with comments and blanks skipped and the value
/// decoded.
fn split_ini_line(line: &str) -> Option<(&str, std::borrow::Cow<'_, str>)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with([';', '#']) {
        return None;
    }
    let (raw_key, raw_value) = line.split_once('=')?;
    Some((raw_key.trim(), decode_ini_value(raw_value.trim())))
}

fn decode_ini_value(value: &str) -> Cow<'_, str> {
    if value.starts_with('\'') && value.ends_with('\'') {
        Cow::Borrowed(
            value.strip_prefix('\'').and_then(|value| value.strip_suffix('\'')).unwrap_or(""),
        )
    } else if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        serde_json::from_str::<String>(value).map_or(Cow::Borrowed(value), Cow::Owned)
    } else {
        Cow::Borrowed(value)
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

impl NpmrcAuth {
    pub fn from_project_ini<Sys: EnvVar>(text: &str, npmrc_dir: &Path) -> Self {
        Self::from_ini_with_options::<Sys>(
            text,
            npmrc_dir,
            ParseOptions { expand_auth_value_env: false, expand_request_destination_env: false },
        )
    }

    /// Parse an `.npmrc` file's contents and pick out the auth/network keys.
    /// Unknown keys are silently dropped. `${VAR}` placeholders inside keys
    /// and values are resolved via the [`EnvVar`] capability; unresolved
    /// placeholders (no env value and no `${VAR:-default}` fallback) are
    /// substituted with `""` and surfaced as warnings. Leaving the literal
    /// `${VAR}` in an auth value would otherwise be sent verbatim — most
    /// damagingly as a bearer auth token under OIDC trusted publishing
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
            let Some((raw_key, raw_value)) = split_ini_line(line) else {
                continue;
            };
            let Some((key, value)) = auth.expand_ini_entry::<Sys>(raw_key, &raw_value, opts) else {
                continue;
            };

            // Capture every auth/scoped/per-registry key verbatim for
            // `pnpm config get` / `list`, independent of the structured
            // parsing below (which only some of these keys feed).
            if crate::config_types::is_ini_config_key(&key) {
                auth.raw_ini_config.insert(key.clone(), value.clone());
            }
            auth.apply_ini_entry(&key, value, npmrc_dir);
        }
        auth
    }

    /// Expand the `${VAR}` placeholders of one entry, or `None` when the
    /// entry is one this source is not trusted to expand.
    ///
    /// Unresolved placeholders become `""` and are recorded as warnings.
    /// Both the raw and the expanded key are checked against the untrusted
    /// sets: a placeholder must not be able to spell its way into a
    /// destination or credential key.
    fn expand_ini_entry<Sys: EnvVar>(
        &mut self,
        raw_key: &str,
        raw_value: &str,
        opts: ParseOptions,
    ) -> Option<(String, String)> {
        if !opts.expand_request_destination_env
            && has_env_placeholder(raw_key)
            && is_request_destination_key(raw_key)
        {
            self.warn_ignored_request_destination_env(raw_key);
            return None;
        }
        if !opts.expand_auth_value_env && has_env_placeholder(raw_key) && is_auth_value_key(raw_key)
        {
            self.warn_ignored_auth_value_env(raw_key);
            return None;
        }
        let (key, key_unresolved) = env_replace_lossy::<Sys>(raw_key);
        if !self.accepts_expanded_entry(raw_key, raw_value, &key, opts) {
            return None;
        }
        let (value, value_unresolved) = env_replace_lossy::<Sys>(raw_value);
        for placeholder in key_unresolved.into_iter().chain(value_unresolved) {
            self.warnings.push(format!("Failed to replace env in config: {placeholder}"));
        }
        Some((key, value))
    }

    /// Whether the entry survives the untrusted-source checks that can only
    /// be made once the key has been expanded.
    fn accepts_expanded_entry(
        &mut self,
        raw_key: &str,
        raw_value: &str,
        key: &str,
        opts: ParseOptions,
    ) -> bool {
        if !opts.expand_request_destination_env
            && has_env_placeholder(raw_key)
            && is_request_destination_key(key)
        {
            self.warn_ignored_request_destination_env(raw_key);
            return false;
        }
        if !opts.expand_auth_value_env && has_env_placeholder(raw_key) && is_auth_value_key(key) {
            self.warn_ignored_auth_value_env(raw_key);
            return false;
        }
        if !opts.expand_request_destination_env
            && has_env_placeholder(raw_value)
            && is_request_destination_value_key(key)
        {
            self.warn_ignored_request_destination_env(key);
            return false;
        }
        if !opts.expand_auth_value_env && has_env_placeholder(raw_value) && is_auth_value_key(key) {
            self.warn_ignored_auth_value_env(key);
            return false;
        }
        true
    }

    /// Record one expanded `key=value` entry in the slot it belongs to.
    pub(super) fn apply_ini_entry(&mut self, key: &str, value: String, npmrc_dir: &Path) {
        if key == "registry" {
            self.registry = Some(value);
            return;
        }
        if let Some(scope) = scoped_registry_key(key) {
            self.scoped_registries.insert(scope.to_string(), normalize_registry_url(&value));
            return;
        }
        if self.apply_network_key(key, &value, npmrc_dir) {
            return;
        }
        if let Some((uri, suffix)) = split_ini_creds_key(key) {
            let entry = self.creds_entry_mut(uri);
            apply_creds_field(entry, suffix, value);
            return;
        }
        if let Some((uri, field, is_file)) = split_ssl_key(key) {
            self.apply_ssl_key(uri, field, is_file, &value);
            return;
        }
        apply_creds_field(&mut self.default_creds, key, value);
    }

    /// The proxy and TLS keys that apply to every request, reporting whether
    /// `key` was one of them.
    pub(super) fn apply_network_key(&mut self, key: &str, value: &str, npmrc_dir: &Path) -> bool {
        match key {
            "https-proxy" => self.https_proxy = Some(value.to_string()),
            "http-proxy" => self.http_proxy = Some(value.to_string()),
            "proxy" => self.legacy_proxy = Some(value.to_string()),
            "no-proxy" | "noproxy" => self.no_proxy = Some(value.to_string()),
            // Repeated `ca=` lines accumulate — multiple values arrive as
            // repeated keys in INI.
            "ca" => self.ca.push(value.to_string()),
            "cafile" => self.cafile = Some(resolve_cafile(value.to_string(), npmrc_dir)),
            "cert" => self.cert = Some(expand_inline_pem(value)),
            "key" => self.key = Some(expand_inline_pem(value)),
            "strict-ssl" => self.strict_ssl = parse_bool(value),
            "local-address" => self.local_address = Some(value.to_string()),
            _ => return false,
        }
        true
    }

    /// A per-registry TLS key. For the `*file` variants the value is a path,
    /// read at parse time (silent on error).
    pub(super) fn apply_ssl_key(&mut self, uri: &str, field: &str, is_file: bool, value: &str) {
        let resolved = if is_file {
            let Ok(contents) = std::fs::read_to_string(value) else {
                return;
            };
            contents
        } else {
            expand_inline_pem(value)
        };
        let entry = self.tls_by_uri.entry(uri.to_owned()).or_default();
        apply_tls_field(entry, field, resolved);
    }

    pub(super) fn warn_ignored_request_destination_env(&mut self, key: &str) {
        self.warnings.push(format!(
            "Ignored project-level request destination {key:?}: environment variables are not expanded in repository-controlled registry or proxy URLs.",
        ));
    }

    pub(super) fn warn_ignored_auth_value_env(&mut self, key: &str) {
        self.warnings.push(format!(
            "Ignored project-level auth setting {key:?}: environment variables are not expanded in repository-controlled registry credentials.",
        ));
    }
}
