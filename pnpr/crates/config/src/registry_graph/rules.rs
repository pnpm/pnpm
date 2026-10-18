use super::super::{
    AccessList, AccessSpec, Ecosystem, IndexMap, PackageAccess, PackagePattern, PackageRule,
    PackageRules, RegistryError, TeamDirectory,
};

/// Compile a `registries:` entry's registry-level `access:` value, naming the
/// registry in the error.
pub(super) fn registry_access_list(
    name: &str,
    spec: Option<&AccessSpec>,
    teams: &TeamDirectory,
) -> Result<Option<AccessList>, RegistryError> {
    spec.map(|spec| {
        let list = spec.to_access_list(teams)?;
        if list.references_owners() {
            return Err("`$owners` is not valid for registry-level `access` rules".to_string());
        }
        Ok(list)
    })
    .transpose()
    .map_err(|reason| RegistryError::InvalidConfig {
        reason: format!("registry {name:?} has an invalid `access` list: {reason}"),
    })
}

/// Compile a concrete registry's `packages:` map into its [`PackageRules`]:
/// keys parsed into the decidable [`PackagePattern`] language, values into
/// per-package permission rules (`{}`/null ⇒ all fields default). Selection
/// is by specificity, so key order carries no meaning; a duplicate key is the
/// only within-registry error, and the YAML parser already rejects literal
/// duplicates in one mapping — the check here guards the graph invariant for
/// any other construction path. Routing-graph checks are handled later by
/// [`Registries::validate`](super::Registries::validate) once the whole graph exists.
fn compile_rule_access_list(
    registry: &str,
    key: &str,
    field: &str,
    spec: Option<&AccessSpec>,
    teams: &TeamDirectory,
) -> Result<Option<AccessList>, RegistryError> {
    let Some(spec) = spec else {
        return Ok(None);
    };
    let list = spec
        .to_access_list(teams)
        .map_err(|reason| RegistryError::InvalidConfig {
            reason: format!(
                "registry {registry:?}: {key:?} has an invalid `{field}` list: {reason}",
            ),
        })?;
    if field == "access" && list.references_owners() {
        return Err(RegistryError::InvalidConfig {
            reason: format!(
                "registry {registry:?}: {key:?} has an invalid `{field}` list: `$owners` is not valid for `access` (read) rules; it is only valid for `publish` and `unpublish`",
            ),
        });
    }
    Ok(Some(list))
}

pub(super) fn build_rules(
    registry: &str,
    ecosystem: Ecosystem,
    packages: &IndexMap<String, Option<PackageAccess>>,
    default_access: Option<AccessList>,
    teams: &TeamDirectory,
) -> Result<PackageRules, RegistryError> {
    let mut rules = Vec::with_capacity(packages.len());
    for (key, rule) in packages {
        let pattern = PackagePattern::parse(key, ecosystem)
            .map_err(|err| RegistryError::InvalidConfig {
                reason: format!("registry {registry:?}: {err}"),
            })?;
        let fields = rule.as_ref();
        rules.push(PackageRule {
            pattern,
            access: compile_rule_access_list(
                registry,
                key,
                "access",
                fields.and_then(|fields| fields.access.as_ref()),
                teams,
            )?,
            publish: compile_rule_access_list(
                registry,
                key,
                "publish",
                fields.and_then(|fields| fields.publish.as_ref()),
                teams,
            )?,
            unpublish: compile_rule_access_list(
                registry,
                key,
                "unpublish",
                fields.and_then(|fields| fields.unpublish.as_ref()),
                teams,
            )?,
        });
    }
    Ok(PackageRules::new(rules, default_access))
}
