use super::{BTreeMap, Cow, Deserialize, HashMap, Serialize};
use crate::{PkgNameVerPeer, Prefix, VersionPart};

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
/// - [`RegistryServerType::Pnpr`] — serves a version's tarball at the package
///   identifier under the registry, `<registry>/<name>@<version>`. The body may
///   be a `.tgz` or a tar; the fetcher tells them apart by content.
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
    Pnpr,
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
    registry_options_by_url
        .get(key.as_ref())
        .copied()
        .unwrap_or_default()
        .server_type
}

/// Whether `registry`'s abbreviated metadata carries the `time` field, per its
/// own declaration or its declared `serverType: pnpr`. [`None`] when neither
/// applies, which leaves the answer to the `registrySupportsTimeField` setting.
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
    let options = registry_options_by_url
        .get(key.as_ref())
        .copied()
        .unwrap_or_default();
    options.supports_time_field.or_else(|| {
        (options.server_type == Some(RegistryServerType::Pnpr)).then_some(true)
    })
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
/// silent, the tarball URL is rebuilt in the wrong layout — and so a
/// new per-registry setting reaches every consumer by being added here.
///
/// The counterpart of the TypeScript CLI's [`RegistryContext`].
#[derive(Debug, Default, Clone)]
pub struct RegistryContext {
    pub registries: HashMap<String, String>,
    /// Every prefix a `<prefix>:` specifier or a registry-qualified key may
    /// name: the declared ones over the built-in aliases.
    pub registries_by_prefix: HashMap<String, String>,
    pub declared_prefixes: DeclaredRegistryPrefixes,
    pub registry_options_by_url: BTreeMap<String, RegistryOptions>,
    /// The scope routes the lockfile's plain keys resolved through, as the
    /// workspace state recorded them (see
    /// `WorkspaceStateSettings::registry_routes`, which says why the default
    /// registry is not among them). A plain key names no registry, so this is
    /// the only record of where its package came from; `None` reads every
    /// plain key as resolved through [`Self::registries`].
    pub locked_registries: Option<HashMap<String, String>>,
}

impl RegistryContext {
    /// `self` reading plain keys as resolved through `locked_registries`.
    #[must_use]
    pub fn with_locked_registries(
        mut self,
        locked_registries: Option<HashMap<String, String>>,
    ) -> Self {
        self.locked_registries = locked_registries;
        self
    }

    /// The registry a `<prefix>:` specifier or a registry-qualified key
    /// names, or `None` when nothing declares `prefix`.
    #[must_use]
    pub fn registry_of_prefix(&self, prefix: &str) -> Option<&str> {
        self.registries_by_prefix.get(prefix).map(String::as_str)
    }

    /// The registry a plain range for `pkg_name` resolves against: the one
    /// its scope routes to, or the default registry.
    #[must_use]
    pub fn routed_registry(&self, pkg_name: &str) -> &str {
        registry_for_package(&self.registries, pkg_name, None)
    }

    /// The registry a plain key for `pkg_name` resolved from: the one its
    /// scope routed to when the lockfile was resolved
    /// ([`Self::locked_registries`]), the default registry for a scope that
    /// had no route then, or the one it routes to now without a record.
    #[must_use]
    pub fn locked_registry(&self, pkg_name: &str) -> &str {
        let Some(locked) = self.locked_registries.as_ref() else {
            return self.routed_registry(pkg_name);
        };
        scope_of(pkg_name)
            .and_then(|scope| locked.get(scope))
            .or_else(|| self.registries.get("default"))
            .map_or("", String::as_str)
    }

    /// The registry a lockfile key's package resolved from, or `None` when
    /// the key is qualified by a prefix nothing declares.
    #[must_use]
    pub fn registry_of_key(&self, key: &PkgNameVerPeer) -> Option<&str> {
        self.registry_of_locked(&key.name.to_string(), key.suffix.registry_qualified())
    }

    /// [`Self::registry_of_key`] for the key of `pkg_name` with the
    /// `(prefix, version)` its version part is qualified by, if any.
    #[must_use]
    pub fn registry_of_locked(
        &self,
        pkg_name: &str,
        registry_qualified: Option<(&str, &node_semver::Version)>,
    ) -> Option<&str> {
        match registry_qualified {
            Some((prefix, _)) => self.registry_of_prefix(prefix),
            None => Some(self.locked_registry(pkg_name)),
        }
    }

    /// The registries a plain key's package resolved from and would resolve
    /// from now, when they differ: its scope was routed to another registry
    /// since the lockfile was resolved. Only a plain semver key moves; a
    /// registry-qualified key names its registry.
    #[must_use]
    pub fn moved_registry(&self, key: &PkgNameVerPeer) -> Option<(&str, &str)> {
        if key.suffix.prefix() != Prefix::None
            || !matches!(key.suffix.version(), VersionPart::Semver(_))
        {
            return None;
        }
        let name = key.name.to_string();
        let from = self.locked_registry(&name);
        let to = self.routed_registry(&name);
        (!same_registry(from, to)).then_some((from, to))
    }

    /// The prefix a key for a package resolved from `registry` carries. See
    /// [`DeclaredRegistryPrefixes`].
    #[must_use]
    pub fn key_prefix_for_registry(&self, registry: &str) -> Option<&str> {
        self.declared_prefixes.prefix_for(registry)
    }

    /// The prefix a key carries for a package a `<prefix>:` specifier
    /// resolved: the one keys for that registry carry, or `prefix` itself
    /// when no declared prefix names its registry. `None` when nothing
    /// declares `prefix`.
    #[must_use]
    pub fn key_prefix_for_alias<'a>(&'a self, prefix: &'a str) -> Option<&'a str> {
        let registry = self.registry_of_prefix(prefix)?;
        Some(self.key_prefix_for_registry(registry).unwrap_or(prefix))
    }

    /// The key this configuration writes for `key`'s package in place of
    /// `key`, when it differs: a plain key for a package whose registry
    /// declares a prefix, or a key qualified by a prefix other than the one
    /// its registry's keys carry. A key qualified by a prefix nothing declares
    /// is left alone; the install reports it.
    #[must_use]
    pub fn rekeyed(&self, key: &PkgNameVerPeer) -> Option<PkgNameVerPeer> {
        if key.suffix.prefix() != Prefix::None {
            return None;
        }
        let (current, version) = match key.suffix.version() {
            VersionPart::Semver(version) => {
                let registry = self.routed_registry(&key.name.to_string());
                (self.key_prefix_for_registry(registry)?, version)
            }
            VersionPart::RegistryQualified { registry_name, version } => {
                let current = self.key_prefix_for_alias(registry_name)?;
                if current == registry_name {
                    return None;
                }
                (current, version)
            }
            VersionPart::File(_) | VersionPart::NonSemver(_) => return None,
        };
        format!("{}@{current}:{version}{}", key.name, key.suffix.peer()).parse().ok()
    }
}

/// The prefixes a project declares, inverted to the registry each names, so
/// the lockfile key of a package can be qualified by the registry it resolved
/// from.
///
/// A key names the registry a package resolved from, not the way its
/// dependency was spelled: a plain range the scope map or the default
/// registry routes and a `<prefix>:` specifier that reach one registry
/// produce one key. Only a prefix the project declares, through
/// `registries.<url>.prefix` or `namedRegistries`, qualifies keys: a project
/// that declares none keeps plain `<name>@<version>` keys, and a built-in
/// alias qualifies only the specifiers that spell it. Several declared
/// prefixes for one registry qualify its keys with the lexicographically
/// smallest one.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DeclaredRegistryPrefixes {
    /// Keyed by [`normalized_registry_key`].
    prefix_by_registry: BTreeMap<String, String>,
}

impl DeclaredRegistryPrefixes {
    /// From the declared `<prefix> → <registry URL>` map. A prefix whose URL
    /// does not parse qualifies nothing.
    pub fn new<Prefix: AsRef<str>, Registry: AsRef<str>>(
        declared: impl IntoIterator<Item = (Prefix, Registry)>,
    ) -> Self {
        let mut prefix_by_registry: BTreeMap<String, String> = BTreeMap::new();
        for (prefix, registry) in declared {
            let Some(key) = normalized_registry_key(registry.as_ref()) else {
                continue;
            };
            let prefix = prefix.as_ref();
            prefix_by_registry
                .entry(key)
                .and_modify(|smallest| {
                    if prefix < smallest.as_str() {
                        prefix.clone_into(smallest);
                    }
                })
                .or_insert_with(|| prefix.to_owned());
        }
        Self { prefix_by_registry }
    }

    /// The prefix keys for packages resolved from `registry` carry.
    #[must_use]
    pub fn prefix_for(&self, registry: &str) -> Option<&str> {
        // Asked once per resolved edge, and most projects declare no prefix.
        if self.prefix_by_registry.is_empty() {
            return None;
        }
        let key = normalized_registry_key(registry)?;
        self.prefix_by_registry.get(&key).map(String::as_str)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.prefix_by_registry.is_empty()
    }
}

/// Whether two registry URLs name one registry, compared by
/// [`normalized_registry_key`], or as written when either does not parse.
#[must_use]
pub fn same_registry(left: &str, right: &str) -> bool {
    match (normalized_registry_key(left), normalized_registry_key(right)) {
        (Some(left), Some(right)) => left == right,
        _ => left == right,
    }
}

/// A registry URL as the server sees it, for telling two spellings of one
/// registry from two registries: the scheme and host case-insensitive, the
/// path as written and ending in a slash. `None` for a URL that does not
/// parse.
#[must_use]
pub fn normalized_registry_key(registry: &str) -> Option<String> {
    let parsed = url::Url::parse(registry).ok()?;
    let mut pathname = parsed.path().to_string();
    if !pathname.ends_with('/') {
        pathname.push('/');
    }
    Some(format!("{}{pathname}", parsed.origin().ascii_serialization()))
}

/// Where a package's tarball lives: the registry it resolved from, and the URL
/// layout that registry serves.
#[derive(Debug, Clone, Copy)]
pub struct TarballUrlOptions<'a> {
    pub registry: &'a str,
    pub server_type: Option<RegistryServerType>,
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
    // The pnpr layout addresses the version by its exact key, build metadata
    // included; `.tgz` filenames drop it.
    if server_type == Some(RegistryServerType::Pnpr) {
        return format!("{registry}{name}@{version}");
    }
    let version = version
        .split_once('+')
        .map(|(base, _)| base)
        .unwrap_or(version);
    // Artifactory keeps the scope in the filename of a scoped package's tarball
    // (`@acme/widget/-/@acme/widget-1.0.0.tgz`); the npm layout strips it.
    let filename_name = match server_type {
        Some(RegistryServerType::Artifactory) => name,
        _ => match name.strip_prefix('@') {
            Some(scoped) => scoped
                .split_once('/')
                .map(|(_, bare)| bare)
                .unwrap_or(name),
            None => name,
        },
    };
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
    registry_for_package(registries, pkg_name, bare_specifier).to_owned()
}

/// [`pick_registry_for_package`], borrowing the URL from `registries`.
#[must_use]
pub fn registry_for_package<'a>(
    registries: &'a HashMap<String, String>,
    pkg_name: &str,
    bare_specifier: Option<&str>,
) -> &'a str {
    let scope = match bare_specifier.and_then(|spec| spec.strip_prefix("npm:")) {
        Some(target) => scope_of(target),
        None => scope_of(pkg_name),
    };
    scope
        .and_then(|scope| registries.get(scope))
        .or_else(|| registries.get("default"))
        .map(String::as_str)
        .unwrap_or("")
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
