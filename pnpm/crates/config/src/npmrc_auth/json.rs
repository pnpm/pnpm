use super::{
    BTreeSet, Config, DEFAULT_REGISTRY_SCOPE, EnvVar, IndexMap, NpmrcAuth, apply_creds_field,
    is_package_scope, nerf_dart, normalize_registry_url, split_creds_key,
};

/// What the config files — the `.npmrc` files as much as the yamls —
/// declared about registry routing, as opposed to what the cascade merely
/// resolved to. Collected before each layer is applied, because applying it
/// is what makes the two indistinguishable by value.
#[derive(Debug, Default, Clone)]
pub struct DeclaredRegistries {
    /// Whether any config file named the registry packages resolve from,
    /// through `registry` or a `registries` entry routing the bare `@`.
    pub registry: bool,
    /// The package scopes any config file routed.
    pub scopes: BTreeSet<String>,
}

impl DeclaredRegistries {
    /// Whether a config file already declared the route for `scope`, whose
    /// `"default"` spelling names the default registry.
    fn covers(&self, scope: &str) -> bool {
        if scope == "default" {
            return self.registry;
        }
        self.scopes.contains(scope)
    }
}

/// Which of `_auth`'s two trusted sources a value came from. They differ in
/// standing, not in shape: the environment is the operator's channel and
/// mandates its routes, while the config file is the user's own store and
/// only fills in what nothing else declares.
#[derive(Clone, Copy)]
enum JsonAuthOrigin {
    Env,
    File,
}

/// The parsed `_auth` setting: registry URL → scope → credentials.
/// Deserialization is strict — any malformed entry (bad JSON, wrong shape,
/// invalid URL/scope, unsupported credential field) is an error, never a
/// silent skip. See [`NpmrcAuth::from_json_sources`].
///
/// [`IndexMap`] preserves source order so a later entry wins for a
/// duplicate inferred route (`"@"` / `@scope` across different hosts) —
/// a `BTreeMap` would re-sort and could pick a different host.
#[derive(Debug, serde::Deserialize)]
struct JsonAuth(IndexMap<JsonAuthRegistry, IndexMap<JsonAuthScope, JsonAuthCreds>>);

/// A registry URL `_auth` key. Validated to be an http(s) URL with no
/// userinfo, query, or fragment (those can carry secrets), then stored
/// normalized (trailing slash) and nerf-darted for the credential key.
/// Parsed with the `url` crate.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Deserialize)]
#[serde(try_from = "String")]
struct JsonAuthRegistry {
    normalized: String,
    nerfed: String,
}

/// Validate a registry URL as an `_auth` key, returning it normalized.
///
/// The single home of the rule, so a writer of the setting — `pnpm login`
/// recording what it was granted — refuses up front exactly what the reader
/// would refuse afterwards, rather than leaving a document that no later
/// command can load.
///
/// Error messages never echo the URL: it can embed secrets in userinfo or a
/// query string, and they reach logs.
pub fn validate_json_auth_registry(value: &str) -> Result<String, String> {
    let Ok(url) = url::Url::parse(value) else {
        return Err("an `_auth` key is not a valid http(s) registry URL".to_string());
    };
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err("an `_auth` registry URL must use http or https".to_string());
    }
    let Some(host) = url.host_str() else {
        return Err("an `_auth` registry URL must have a host".to_string());
    };
    // A credential-free label for the remaining messages.
    let label = match url.port() {
        Some(port) => format!("{}://{host}:{port}", url.scheme()),
        None => format!("{}://{host}", url.scheme()),
    };
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(format!(
            "registry URL {label} must not include credentials, a query, or a fragment",
        ));
    }
    let normalized = normalize_registry_url(url.as_str());
    if nerf_dart(&normalized).is_empty() {
        return Err(format!("registry URL {label} is not a valid registry URL"));
    }
    Ok(normalized)
}

/// Whether `scope` is a key `_auth` accepts: the bare `@` standing for the
/// registry itself, or a package scope such as `@org`. Shares its home with
/// [`validate_json_auth_registry`] for the same reason.
#[must_use]
pub fn is_json_auth_scope(scope: &str) -> bool {
    scope == DEFAULT_REGISTRY_SCOPE || is_package_scope(scope)
}

impl TryFrom<String> for JsonAuthRegistry {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let normalized = validate_json_auth_registry(&value)?;
        let nerfed = nerf_dart(&normalized);
        Ok(JsonAuthRegistry { normalized, nerfed })
    }
}

/// A scope key within a registry: `@` for registry-wide/default credentials,
/// or a package scope like `@org`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Deserialize)]
#[serde(try_from = "String")]
enum JsonAuthScope {
    Default,
    Package(String),
}

impl TryFrom<String> for JsonAuthScope {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value == DEFAULT_REGISTRY_SCOPE {
            return Ok(JsonAuthScope::Default);
        }
        if is_package_scope(&value) {
            return Ok(JsonAuthScope::Package(value));
        }
        Err(format!(r#"scope "{value}" must be "@" or a package scope like "@org""#))
    }
}

/// Credentials for one registry scope. Only `authToken` is accepted; the
/// deprecated `basicAuth` / `username` + `password` forms are rejected via
/// `deny_unknown_fields`.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct JsonAuthCreds {
    #[serde(rename = "authToken")]
    auth_token: String,
}

impl NpmrcAuth {
    /// Parse the structured `_auth` setting from its two trusted, non-repo
    /// sources — the global pnpm `config.yaml` (`global_value`) and the
    /// `pnpm_config__auth` env var — global-first then env, so the env var
    /// wins on conflict.
    ///
    /// Parsing is strict: a malformed value (bad JSON, wrong shape, invalid
    /// registry URL or scope, an unsupported credential field) is a hard
    /// error, not a warning — both sources are user-controlled, so a typo
    /// should surface immediately rather than silently drop auth.
    ///
    /// The env var exists because GitHub Actions / bash / zsh drop env var
    /// names containing `/`, `:`, or `.`, breaking the
    /// `pnpm_config_//host/:_authToken=…` form on CI (pnpm/pnpm#12314).
    /// Values are used as-is — no `${VAR}` re-expansion, which would let
    /// repo-controlled env vars leak into them.
    pub fn from_json_sources<Sys: EnvVar>(
        global_value: Option<&serde_json::Value>,
    ) -> Result<Self, serde_json::Error> {
        let mut auth = NpmrcAuth::default();
        if let Some(global_value) = global_value {
            auth.apply_json_auth(
                serde_json::from_value(global_value.clone())?,
                JsonAuthOrigin::File,
            );
        }
        // Lowercase is the documented form; UPPER covers the all-caps shell
        // convention some CI runners apply.
        let env_value = Sys::var("pnpm_config__auth")
            .filter(|value| !value.is_empty())
            .or_else(|| Sys::var("PNPM_CONFIG__AUTH").filter(|value| !value.is_empty()));
        if let Some(value) = env_value {
            auth.apply_json_auth(serde_json::from_str(&value)?, JsonAuthOrigin::Env);
        }
        Ok(auth)
    }

    /// Fold a parsed [`JsonAuth`] into `self` (last-write-wins, so the env
    /// object applied after the global one overrides on conflict): each
    /// entry becomes a `//host/:_authToken` credential and an inferred
    /// registry route (see [`Self::json_env_registries`]).
    fn apply_json_auth(&mut self, parsed: JsonAuth, origin: JsonAuthOrigin) {
        for (registry, scopes) in parsed.0 {
            for (scope, creds) in scopes {
                self.apply_json_entry(&registry, scope, creds.auth_token, origin);
            }
        }
    }

    /// One `registry → scope → token` entry of a parsed [`JsonAuth`].
    fn apply_json_entry(
        &mut self,
        registry: &JsonAuthRegistry,
        scope: JsonAuthScope,
        auth_token: String,
        origin: JsonAuthOrigin,
    ) {
        let key = match &scope {
            JsonAuthScope::Default => format!("{}:_authToken", registry.nerfed),
            JsonAuthScope::Package(scope) => format!("{}:{scope}:_authToken", registry.nerfed),
        };
        if let Some((uri, suffix)) = split_creds_key(&key) {
            let entry = self.creds_entry_mut(uri);
            apply_creds_field(entry, suffix, auth_token);
        }
        let route_key = match scope {
            JsonAuthScope::Default => "default".to_string(),
            JsonAuthScope::Package(scope) => scope,
        };
        let routes = match origin {
            JsonAuthOrigin::Env => &mut self.json_env_registries,
            JsonAuthOrigin::File => &mut self.json_file_registries,
        };
        routes.insert(route_key, registry.normalized.clone());
    }

    /// Apply the [`Self::json_env_registries`] routes. Unlike
    /// [`Self::apply_registry_and_warn`] (which runs *before* workspace
    /// yaml), this is called *after* yaml so the inferred routes win over
    /// repo-controlled registries.
    ///
    /// The file-sourced routes fill in only what a config file has not
    /// declared, which `declared` names: provenance rather than value,
    /// because pinning the registry a lower layer already resolved to is
    /// still a declaration, while the builtin default is not one. The
    /// environment-sourced routes replace whatever they find.
    pub fn apply_json_env_registries(
        &mut self,
        config: &mut Config,
        declared: &DeclaredRegistries,
    ) {
        let file_routes = std::mem::take(&mut self.json_file_registries);
        for (scope, url) in file_routes.into_iter().filter(|(scope, _)| !declared.covers(scope)) {
            if scope == "default" {
                config.registry.clone_from(&url);
                continue;
            }
            config.registries_by_scope.insert(scope, url);
        }
        for (scope, url) in std::mem::take(&mut self.json_env_registries) {
            if scope == "default" {
                config.registry.clone_from(&url);
            }
            config.registries_by_scope.insert(scope, url);
        }
    }
}
