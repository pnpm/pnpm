pub(super) use namespace::{
    ecosystem_package_keys, org_collision_error, validate_org_namespace, validate_registry_key,
    validate_registry_name,
};

mod namespace;

use super::{
    AccessList, AccessSpec, DefaultRegistryFile, Ecosystem, EnvVar, HostedConfig, HostedFile,
    IndexMap, PackageAccess, PackagePattern, PackageRule, PackageRules, Registries, Registry,
    RegistryConfigError, RegistryError, RegistryFile, RegistryGroupFile, SystemEnv, Teams,
    UpstreamConfig, UpstreamConfigFile, UpstreamFile, build_teams, registry_mock_rules,
    resolve_upstream_config,
};

/// Turn a static [`RegistryConfigError`] into the server-wide config error so a
/// bad registry set fails startup and config reload like any other.
pub(super) fn registry_err(err: &RegistryConfigError) -> RegistryError {
    RegistryError::InvalidConfig { reason: err.to_string() }
}

pub(super) fn set_group_ecosystem(
    name: &str,
    declared: &mut Option<Ecosystem>,
    ecosystem: Ecosystem,
) -> Result<(), RegistryError> {
    if declared.is_some_and(|declared| declared != ecosystem) {
        return Err(RegistryError::InvalidConfig {
            reason: format!(
                "registry {name:?} declares an ecosystem different from its {ecosystem} group",
            ),
        });
    }
    *declared = Some(ecosystem);
    Ok(())
}

pub(super) fn flatten_registry_groups(
    groups: IndexMap<String, RegistryGroupFile>,
) -> Result<IndexMap<String, RegistryFile>, RegistryError> {
    let mut entries = IndexMap::new();
    for (name, group) in groups {
        match group {
            RegistryGroupFile::Registry(registry) => {
                validate_registry_name(&name)?;
                entries.insert(name, registry);
            }
            RegistryGroupFile::Ecosystem(registries) => {
                let ecosystem = Ecosystem::all()
                    .find(|ecosystem| ecosystem.as_str() == name)
                    .ok_or_else(|| RegistryError::InvalidConfig {
                        reason: format!("unknown registry ecosystem {name:?}"),
                    })?;
                flatten_ecosystem_group(ecosystem, registries, &mut entries)?;
            }
        }
    }
    Ok(entries)
}

/// Qualify every registry declared under an `<ecosystem>:` group with that
/// ecosystem, in its own identity and in the sources a router names.
pub(super) fn flatten_ecosystem_group(
    ecosystem: Ecosystem,
    registries: IndexMap<String, RegistryFile>,
    entries: &mut IndexMap<String, RegistryFile>,
) -> Result<(), RegistryError> {
    for (name, mut registry) in registries {
        validate_registry_name(&name)?;
        match &mut registry {
            RegistryFile::Hosted(hosted) => {
                set_group_ecosystem(&name, &mut hosted.ecosystem, ecosystem)?;
                hosted.org.get_or_insert_with(|| format!("{ecosystem}~{name}"));
            }
            RegistryFile::Upstream(upstream) => {
                set_group_ecosystem(&name, &mut upstream.ecosystem, ecosystem)?;
            }
            RegistryFile::Router(router) => {
                for source in &mut router.sources {
                    validate_registry_name(source)?;
                    *source = format!("{ecosystem}/{source}");
                }
            }
        }
        entries.insert(format!("{ecosystem}/{name}"), registry);
    }
    Ok(())
}

/// Build the validated [`Registries`] graph (and the hosted table) from the
/// resolved upstreams and the `registries:` block. Every upstream is an upstream
/// registry; `registries:` adds hosted, further upstream, and router registries.
/// Upstream registries declared under `registries:` are folded into `upstreams` so they
/// reuse the same serving and route-classification machinery. Fails closed on
/// any name collision, malformed registry, or invalid routing graph.
///
/// With `resolve_upstreams` false (the registry surface is disabled), an
/// upstream registry still joins the graph for validation but its credentials
/// and serving config are not resolved — a resolver-only tier must not fail
/// on (or carry) upstream secrets it never uses.
pub(super) fn build_registries(
    upstreams: &mut IndexMap<String, UpstreamConfig>,
    registry_files: IndexMap<String, RegistryGroupFile>,
    default_registry: Option<DefaultRegistryFile>,
    resolve_upstreams: bool,
) -> Result<(IndexMap<String, HostedConfig>, Registries), RegistryError> {
    let registry_files = flatten_registry_groups(registry_files)?;
    let (default_registry, defaults) = resolve_default_registry(default_registry)?;
    let mut builder = RegistryGraphBuilder::default();
    // Every configured upstream is, by definition, an upstream registry addressable
    // at `/~<name>/`. No declared patterns ⇒ it serves every name.
    for name in upstreams.keys() {
        validate_registry_name(name)?;
        builder.graph.insert(name.clone(), Registry::Upstream { patterns: Vec::new() });
    }
    for (name, file) in registry_files {
        builder.add(name, file, upstreams, resolve_upstreams)?;
    }
    let registries =
        builder.ecosystems.iter().filter(|(_, ecosystem)| **ecosystem != Ecosystem::Npm).fold(
            Registries::new(builder.graph, default_registry),
            |registries, (name, ecosystem)| registries.with_ecosystem(name, *ecosystem),
        );
    let defaults = addressed_defaults(defaults, &registries);
    let registries = registries.with_defaults(defaults);
    registries.validate().map_err(|err| registry_err(&err))?;
    Ok((builder.hosted, registries))
}

/// The shared default registry, or the per-ecosystem defaults qualified
/// by their ecosystem.
pub(super) fn resolve_default_registry(
    default_registry: Option<DefaultRegistryFile>,
) -> Result<(Option<String>, IndexMap<Ecosystem, String>), RegistryError> {
    match default_registry {
        Some(DefaultRegistryFile::Shared(name)) => Ok((Some(name), IndexMap::new())),
        Some(DefaultRegistryFile::Ecosystems(defaults)) => {
            let defaults = defaults
                .into_iter()
                .map(|(ecosystem, name)| {
                    validate_registry_name(&name)?;
                    Ok((ecosystem, format!("{ecosystem}/{name}")))
                })
                .collect::<Result<IndexMap<_, _>, RegistryError>>()?;
            Ok((None, defaults))
        }
        None => Ok((None, IndexMap::new())),
    }
}

/// The registry graph under construction: the hosted registries' configs,
/// every registry's routing entry and the ecosystem each serves.
#[derive(Default)]
pub(super) struct RegistryGraphBuilder {
    pub(super) hosted: IndexMap<String, HostedConfig>,
    pub(super) graph: IndexMap<String, Registry>,
    pub(super) ecosystems: IndexMap<String, Ecosystem>,
}

impl RegistryGraphBuilder {
    pub(super) fn add(
        &mut self,
        name: String,
        file: RegistryFile,
        upstreams: &mut IndexMap<String, UpstreamConfig>,
        resolve_upstreams: bool,
    ) -> Result<(), RegistryError> {
        validate_registry_key(&name)?;
        if self.graph.contains_key(&name) {
            return Err(RegistryError::InvalidConfig {
                reason: format!(
                    "registry {name:?} collides with another registry or upstream of the same name",
                ),
            });
        }
        match file {
            RegistryFile::Hosted(registry) => {
                let (config, ecosystem, patterns) =
                    build_hosted_entry(&name, registry, &self.hosted)?;
                self.hosted.insert(name.clone(), config);
                self.ecosystems.insert(name.clone(), ecosystem);
                self.graph.insert(name, Registry::Hosted { patterns });
            }
            RegistryFile::Upstream(upstream) => {
                let (resolved, ecosystem, patterns) =
                    build_upstream_entry(&name, *upstream, resolve_upstreams)?;
                self.ecosystems.insert(name.clone(), ecosystem);
                if let Some(resolved) = resolved {
                    upstreams.insert(name.clone(), resolved);
                }
                self.graph.insert(name, Registry::Upstream { patterns });
            }
            RegistryFile::Router(router) => {
                self.graph.insert(name, Registry::Router { sources: router.sources });
            }
        }
        Ok(())
    }
}

/// Each ecosystem's default, addressed the way the registries name it.
pub(super) fn addressed_defaults(
    defaults: IndexMap<Ecosystem, String>,
    registries: &Registries,
) -> IndexMap<Ecosystem, String> {
    defaults
        .into_iter()
        .map(|(ecosystem, target)| {
            let local = Registries::local_name(&target);
            let key = registries.addressed(local, ecosystem).unwrap_or(&target).to_string();
            (ecosystem, key)
        })
        .collect()
}

/// The serving config, ecosystem and claimed patterns of one `hosted:` registry.
pub(super) fn build_hosted_entry(
    name: &str,
    registry: HostedFile,
    hosted: &IndexMap<String, HostedConfig>,
) -> Result<(HostedConfig, Ecosystem, Vec<PackagePattern>), RegistryError> {
    let org = registry.org.unwrap_or_default();
    validate_org_namespace(name, &org)?;
    if let Some((other, _)) =
        hosted.iter().find(|(_, existing): &(_, &HostedConfig)| existing.org == org)
    {
        return Err(org_collision_error(name, &org, other));
    }
    let ecosystem = registry.ecosystem.unwrap_or_default();
    let teams = build_teams(name, &registry.teams)?;
    let access = registry_access_list(name, registry.access.as_ref(), &teams)?;
    let packages = ecosystem_package_keys(name, ecosystem, registry.packages)?;
    let rules = build_rules(name, ecosystem, &packages, access, &teams)?;
    let patterns = rules.patterns();
    Ok((HostedConfig { org, rules, teams }, ecosystem, patterns))
}

/// The resolved serving config, ecosystem and claimed patterns of one
/// `upstream:` registry. The rules are built before the `resolve_upstreams`
/// fork so the graph carries the namespace on every tier, and so a
/// `publish`/`unpublish` value — a write rule on a registry no write can land
/// on — fails startup on every tier too. A resolver-only tier gets no serving
/// config back.
pub(super) fn build_upstream_entry(
    name: &str,
    upstream: UpstreamFile,
    resolve_upstreams: bool,
) -> Result<(Option<UpstreamConfig>, Ecosystem, Vec<PackagePattern>), RegistryError> {
    let ecosystem = upstream.ecosystem.unwrap_or_default();
    // The registry-level default the rules fall back to: the upstream's
    // `access:` gate, or `$all` for a public origin.
    let teams = build_teams(name, &upstream.teams)?;
    let access = registry_access_list(name, upstream.access.as_ref(), &teams)?;
    let packages = ecosystem_package_keys(name, ecosystem, upstream.packages.clone())?;
    let rules = build_rules(name, ecosystem, &packages, access, &teams)?;
    if rules.refines_writes() {
        return Err(RegistryError::InvalidConfig {
            reason: format!(
                "upstream registry {name:?} declares `publish`/`unpublish` rules in \
                 its `packages:` map; writes can never land on an upstream",
            ),
        });
    }
    let patterns = rules.patterns();
    if !resolve_upstreams {
        return Ok((None, ecosystem, patterns));
    }
    let mut resolved = resolve_upstream_registry::<SystemEnv>(name, upstream, &teams)?;
    resolved.rules = rules;
    Ok((Some(resolved), ecosystem, patterns))
}

/// Compile a `registries:` entry's registry-level `access:` value, naming the
/// registry in the error.
pub(super) fn registry_access_list(
    name: &str,
    spec: Option<&AccessSpec>,
    teams: &Teams,
) -> Result<Option<AccessList>, RegistryError> {
    spec.map(|spec| spec.to_access_list(teams)).transpose().map_err(|reason| {
        RegistryError::InvalidConfig {
            reason: format!("registry {name:?} has an invalid `access` list: {reason}"),
        }
    })
}

/// Compile a concrete registry's `packages:` map into its [`PackageRules`]:
/// keys parsed into the decidable [`PackagePattern`] language, values into
/// per-package permission rules (`{}`/null ⇒ all fields default). Selection
/// is by specificity, so key order carries no meaning; a duplicate key is the
/// only within-registry error, and the YAML parser already rejects literal
/// duplicates in one mapping — the check here guards the graph invariant for
/// any other construction path. Routing-graph checks are handled later by
/// [`Registries::validate`] once the whole graph exists.
pub(super) fn build_rules(
    registry: &str,
    ecosystem: Ecosystem,
    packages: &IndexMap<String, Option<PackageAccess>>,
    default_access: Option<AccessList>,
    teams: &Teams,
) -> Result<PackageRules, RegistryError> {
    let rules = packages
        .iter()
        .map(|(key, rule)| {
            let pattern = PackagePattern::parse(key, ecosystem).map_err(|err| {
                RegistryError::InvalidConfig { reason: format!("registry {registry:?}: {err}") }
            })?;
            let fields = rule.as_ref();
            let list = |field: &str, spec: Option<&AccessSpec>| {
                spec.map(|spec| spec.to_access_list(teams)).transpose().map_err(|reason| {
                    RegistryError::InvalidConfig {
                        reason: format!(
                            "registry {registry:?}: {key:?} has an invalid `{field}` list: {reason}",
                        ),
                    }
                })
            };
            Ok(PackageRule {
                pattern,
                access: list("access", fields.and_then(|fields| fields.access.as_ref()))?,
                publish: list("publish", fields.and_then(|fields| fields.publish.as_ref()))?,
                unpublish: list("unpublish", fields.and_then(|fields| fields.unpublish.as_ref()))?,
            })
        })
        .collect::<Result<Vec<_>, RegistryError>>()?;
    Ok(PackageRules::new(rules, default_access))
}

/// Resolve an `upstream:` registry into the shared [`UpstreamConfig`] runtime shape.
/// A `public` upstream is anonymous and world-readable (no credential, no
/// access gate); a non-`public` one must declare `access:` naming who may
/// reach it at `/~<name>/`. Declaring both `public` and `auth` is rejected —
/// a public origin sends no credential.
pub(super) fn resolve_upstream_registry<Sys: EnvVar>(
    name: &str,
    file: UpstreamFile,
    teams: &Teams,
) -> Result<UpstreamConfig, RegistryError> {
    validate_upstream_access(name, &file)?;
    let access = if file.public { None } else { file.access };
    let upstream_config_file = UpstreamConfigFile {
        url: file.url,
        auth: file.auth,
        headers: file.headers,
        maxage: file.maxage,
        timeout: file.timeout,
        max_fails: file.max_fails,
        fail_timeout: file.fail_timeout,
        cache: file.cache,
        search: file.search,
        access,
    };
    resolve_upstream_config::<Sys>(name, upstream_config_file, teams)
}

pub(super) fn registry_mock_graph() -> (IndexMap<String, HostedConfig>, Registries) {
    let rules = registry_mock_rules();
    let local_patterns = rules.patterns();
    let mut hosted = IndexMap::new();
    hosted.insert(
        "local".to_string(),
        HostedConfig { org: String::new(), rules, teams: Teams::default() },
    );
    let graph = [
        ("local".to_string(), Registry::Hosted { patterns: local_patterns }),
        ("npmjs".to_string(), Registry::Upstream { patterns: Vec::new() }),
        (
            "main".to_string(),
            Registry::Router { sources: vec!["local".to_string(), "npmjs".to_string()] },
        ),
    ];
    let registries = Registries::new(graph.into_iter().collect(), Some("main".to_string()));
    (hosted, registries)
}

/// Public origins must reject credentials and access gates rather than silently
/// exposing private data or sending credentials to anonymous origins.
pub(super) fn validate_upstream_access(
    name: &str,
    file: &UpstreamFile,
) -> Result<(), RegistryError> {
    if file.public && file.auth.is_some() {
        return Err(RegistryError::InvalidConfig {
            reason: format!(
                "upstream registry {name:?} is `public` but also declares `auth`; a public origin \
                 sends no credential",
            ),
        });
    }
    if file.public && file.access.is_some() {
        return Err(RegistryError::InvalidConfig {
            reason: format!(
                "upstream registry {name:?} is `public` but also declares `access`; a public origin \
                 is reachable anonymously",
            ),
        });
    }
    if file.public && !file.headers.is_empty() {
        // A public origin is fetched anonymously, so it sends no request headers
        // at all. Rejecting *any* custom header (not just `Authorization`) closes
        // the door on a credential smuggled through `X-Api-Key`, a cookie, or any
        // other header on a registry that is meant to be reachable anonymously.
        return Err(RegistryError::InvalidConfig {
            reason: format!(
                "upstream registry {name:?} is `public` but declares custom `headers`; a public \
                 origin is fetched anonymously and sends none",
            ),
        });
    }
    if !file.public && file.access.is_none() {
        return Err(RegistryError::InvalidConfig {
            reason: format!(
                "upstream registry {name:?} must set `public: true` or declare `access:` (who may \
                 reach it at /~{name}/)",
            ),
        });
    }
    Ok(())
}

pub(super) struct ResolvedFileRegistries {
    pub(super) upstreams: IndexMap<String, UpstreamConfig>,
    pub(super) hosted: IndexMap<String, HostedConfig>,
    pub(super) registries: Registries,
}

pub(super) fn resolve_file_registries(
    registries: IndexMap<String, RegistryGroupFile>,
    default_registry: Option<DefaultRegistryFile>,
    registry_enabled: bool,
) -> Result<ResolvedFileRegistries, RegistryError> {
    let mut upstreams: IndexMap<String, UpstreamConfig> = IndexMap::new();
    let (hosted, registries) =
        build_registries(&mut upstreams, registries, default_registry, registry_enabled)?;
    Ok(ResolvedFileRegistries { upstreams, hosted, registries })
}
