use super::{
    BTreeMap, Cow, Deserialize, HashMap, Integrity, Serialize, integrity_addressed_tarball_path,
};

/// The software serving a registry, declared through the `registries`
/// setting. Modeled as an [`Option`] everywhere it is threaded, because
/// "behaves like the npm registry" is a claim only the operator can make:
///
/// - [`None`] — strict. Only the exact canonical URL is reconstructible. This
///   is how every registry but registry.npmjs.org is read by default.
/// - [`RegistryServerType::Npm`] — behaves like registry.npmjs.org, which
///   serves a scoped package from the percent-encoded path as well as the
///   unencoded one. A faithful mirror or caching proxy of it is this.
/// - [`RegistryServerType::Artifactory`] — repeats the scope in a scoped
///   package's tarball filename.
///
/// Only layouts pnpm can rebuild a URL for belong here. A registry that serves
/// tarballs from a content-derived path (GitHub Packages
/// `/download/<scope>/<name>/<version>/<sha256>`) has no variant: the digest is
/// a fact about the bytes rather than about the package's identity, so its URLs
/// are kept in the lockfile instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RegistryServerType {
    Npm,
    Artifactory,
}

/// Non-secret, per-registry settings from the `registries` setting.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RegistryOptions {
    #[serde(default)]
    pub server_type: Option<RegistryServerType>,
    /// Whether this registry's abbreviated metadata carries the `time` field.
    ///
    /// `registry.npmjs.org` does not, which is why the default is `false` and
    /// why a time-based resolution falls back to the far larger full metadata.
    /// A registry that does carry it is worth declaring: the fallback is per
    /// registry, so one that needs full metadata no longer costs it at the
    /// others.
    #[serde(default)]
    pub supports_time_field: Option<bool>,
}

/// registry.npmjs.org is the one registry whose layout pnpm knows without
/// being told, so it is a row of data rather than a hostname comparison. A
/// declared server type wins over it.
const DEFAULT_REGISTRY_SERVER_TYPES: &[(&str, RegistryServerType)] =
    &[("https://registry.npmjs.org/", RegistryServerType::Npm)];

/// The layout the user declared for `registry`, or [`None`] for none.
///
/// Built-in layouts are deliberately not applied here — they belong with the
/// predicate that acts on them, so that a code path which never threads
/// `registryOptions` still gets them.
///
/// `registry_options_by_url` is keyed by registry URL with a trailing slash, the way
/// the config reader normalizes it, so the lookup normalizes its query to
/// match.
#[must_use]
pub fn registry_server_type(
    registry_options_by_url: &BTreeMap<String, RegistryOptions>,
    registry: &str,
) -> Option<RegistryServerType> {
    let key = if registry.ends_with('/') {
        Cow::Borrowed(registry)
    } else {
        Cow::Owned(format!("{registry}/"))
    };
    registry_options_by_url.get(key.as_ref()).copied().unwrap_or_default().server_type
}

/// Whether `registry`'s abbreviated metadata carries the `time` field, per its
/// own declaration. [`None`] when it does not declare one, which leaves the
/// answer to the `registrySupportsTimeField` setting.
///
/// Keyed like [`registry_server_type`], which is why the query is normalized
/// the same way.
#[must_use]
pub fn registry_supports_time_field(
    registry_options_by_url: &BTreeMap<String, RegistryOptions>,
    registry: &str,
) -> Option<bool> {
    let key = if registry.ends_with('/') {
        Cow::Borrowed(registry)
    } else {
        Cow::Owned(format!("{registry}/"))
    };
    registry_options_by_url.get(key.as_ref()).copied().unwrap_or_default().supports_time_field
}

/// A declared server type wins; otherwise the built-in layout of a known
/// registry applies, and an unknown registry is read strictly.
fn effective_server_type(opts: TarballUrlOptions<'_>) -> Option<RegistryServerType> {
    opts.server_type.or_else(|| {
        let registry = if opts.registry.ends_with('/') {
            Cow::Borrowed(opts.registry)
        } else {
            Cow::Owned(format!("{}/", opts.registry))
        };
        DEFAULT_REGISTRY_SERVER_TYPES
            .iter()
            .find(|(default_registry, _)| *default_registry == registry.as_ref())
            .map(|(_, server_type)| *server_type)
    })
}

/// Everything needed to decide which registry a package came from and what
/// that registry does: the scope-routed URLs, the `<name>:`-addressed aliases,
/// and the declared per-registry settings.
///
/// Threaded as one value rather than as three parameters so a consumer cannot
/// be handed the routing without the settings — dropping the settings is
/// silent, the tarball URL is simply rebuilt in the wrong layout — and so a
/// new per-registry setting reaches every consumer by being added here.
///
/// The counterpart of the TypeScript CLI's [`RegistryContext`].
#[derive(Debug, Default, Clone)]
pub struct RegistryContext {
    pub registries: HashMap<String, String>,
    /// As the user wrote it; built-in aliases are merged in at lookup.
    pub registries_by_prefix: HashMap<String, String>,
    pub registry_options_by_url: BTreeMap<String, RegistryOptions>,
}

/// Where a package's tarball lives: the registry it resolved from, and the URL
/// layout that registry serves.
#[derive(Debug, Clone, Copy)]
pub struct TarballUrlOptions<'a> {
    pub registry: &'a str,
    pub server_type: Option<RegistryServerType>,
}

/// Build an integrity-addressed tarball URL relative to `registry`.
#[must_use]
pub fn integrity_addressed_registry_tarball_url(
    integrity: &Integrity,
    registry: &str,
) -> Option<String> {
    let path = integrity_addressed_tarball_path(integrity)?;
    let registry =
        if registry.ends_with('/') { registry.to_string() } else { format!("{registry}/") };
    url::Url::parse(&registry).ok()?.join(&path).ok().map(Into::into)
}

/// Whether `tarball` is the exact digest route derived from `registry` and `integrity`.
#[must_use]
pub fn is_integrity_addressed_registry_tarball_url(
    tarball: &str,
    integrity: &Integrity,
    registry: &str,
) -> bool {
    if !tarball.contains("/-/tarballs/sha512/") {
        return false;
    }
    let Some(expected) = integrity_addressed_registry_tarball_url(integrity, registry) else {
        return false;
    };
    match (url::Url::parse(tarball), url::Url::parse(&expected)) {
        (Ok(actual), Ok(expected)) => actual == expected,
        _ => false,
    }
}

/// Derive the canonical npm registry tarball URL for `name@version`. Port of
/// the [`get-npm-tarball-url`](https://www.npmjs.com/package/get-npm-tarball-url)
/// package pnpm uses.
///
/// This is the single source of the URL shape: the lockfile writer drops a
/// tarball URL only when this function rebuilds it, and the lockfile reader
/// rebuilds it with this function. Both sides therefore agree by construction,
/// including under a non-npm [`RegistryServerType`].
#[must_use]
pub fn npm_tarball_url(name: &str, version: &str, opts: TarballUrlOptions<'_>) -> String {
    let TarballUrlOptions { registry, server_type } = opts;
    let registry =
        if registry.ends_with('/') { registry.to_string() } else { format!("{registry}/") };
    // Artifactory keeps the scope in the filename of a scoped package's tarball
    // (`@acme/widget/-/@acme/widget-1.0.0.tgz`); the npm layout strips it.
    let filename_name = match server_type {
        Some(RegistryServerType::Artifactory) => name,
        Some(RegistryServerType::Npm) | None => match name.strip_prefix('@') {
            Some(scoped) => scoped.split_once('/').map_or(name, |(_, bare)| bare),
            None => name,
        },
    };
    let version = version.split_once('+').map_or(version, |(base, _)| base);
    format!("{registry}{name}/-/{filename_name}-{version}.tgz")
}

/// Whether `tarball` is the URL [`npm_tarball_url`] rebuilds for `name` and
/// `version` — i.e. it can be dropped from the lockfile and rebuilt on demand.
pub(super) fn is_canonical_registry_tarball_url(
    tarball: &str,
    name: &str,
    version: &str,
    opts: TarballUrlOptions<'_>,
) -> bool {
    let expected = npm_tarball_url(name, version, opts);
    let expected = remove_protocol(&expected);
    let actual = remove_protocol(tarball);
    // A registry behaving like registry.npmjs.org serves a scoped package from
    // both the encoded and the unencoded path. A registry that has not been
    // declared to behave like it may serve only the encoded one, so its URL is
    // kept. See <https://github.com/pnpm/pnpm/issues/13534>.
    expected == actual
        || (effective_server_type(opts) == Some(RegistryServerType::Npm)
            && expected == actual.replace("%2f", "/").replace("%2F", "/"))
}

/// Default-vs-scope routing for an npm package.
///
/// Routing rules:
///
/// 1. **`npm:` alias.** When `bare_specifier` is an `npm:` alias the
///    *alias target* decides routing, not the local key:
///    - `npm:@scope/name@<spec>` → `registries[@scope]`.
///    - `npm:name@<spec>` (unscoped target) → `registries["default"]`,
///      never the local alias's scope, because the fetched package is
///      unscoped and doesn't live on a scoped registry.
/// 2. **Plain spec.** Falls back to `pkg_name`'s scope when present;
///    otherwise `registries["default"]`.
#[must_use]
pub fn pick_registry_for_package(
    registries: &HashMap<String, String>,
    pkg_name: &str,
    bare_specifier: Option<&str>,
) -> String {
    let scope = match bare_specifier.and_then(|spec| spec.strip_prefix("npm:")) {
        Some(target) => scope_of(target),
        None => scope_of(pkg_name),
    };
    if let Some(scope) = scope
        && let Some(url) = registries.get(scope)
    {
        return url.clone();
    }
    registries.get("default").cloned().unwrap_or_default()
}

fn scope_of(name: &str) -> Option<&str> {
    if !name.starts_with('@') {
        return None;
    }
    name.find('/').map(|sep| &name[..sep])
}

/// Strip only a leading `http://` or `https://` scheme (case-insensitive) so
/// URLs are compared protocol-insensitively, without truncating on a later
/// `://` in the path or query.
fn remove_protocol(url: &str) -> &str {
    ["https://", "http://"]
        .into_iter()
        .find_map(|scheme| {
            url.get(..scheme.len())
                .filter(|head| head.eq_ignore_ascii_case(scheme))
                .map(|_| &url[scheme.len()..])
        })
        .unwrap_or(url)
}
