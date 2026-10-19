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

fn validate_unpublish_supported(
    registry: &str,
    ecosystem: Ecosystem,
    key: &str,
    rule: Option<&PackageAccess>,
) -> Result<(), RegistryError> {
    if (ecosystem == Ecosystem::Cargo || ecosystem == Ecosystem::Pypi)
        && rule.and_then(|fields| fields.unpublish.as_ref()).is_some()
    {
        return Err(RegistryError::InvalidConfig {
            reason: format!(
                "registry {registry:?} serves {ecosystem}, which does not support unpublishing; \
                 `unpublish` rule on {key:?} is invalid",
            ),
        });
    }
    Ok(())
}

fn compile_package_rule(
    registry: &str,
    ecosystem: Ecosystem,
    key: &str,
    rule: Option<&PackageAccess>,
    teams: &TeamDirectory,
) -> Result<PackageRule, RegistryError> {
    let pattern = PackagePattern::parse(key, ecosystem)
        .map_err(|err| RegistryError::InvalidConfig {
            reason: format!("registry {registry:?}: {err}"),
        })?;
    validate_unpublish_supported(registry, ecosystem, key, rule)?;
    Ok(PackageRule {
        pattern,
        access: compile_rule_access_list(
            registry,
            key,
            "access",
            rule.and_then(|fields| fields.access.as_ref()),
            teams,
        )?,
        publish: compile_rule_access_list(
            registry,
            key,
            "publish",
            rule.and_then(|fields| fields.publish.as_ref()),
            teams,
        )?,
        unpublish: compile_rule_access_list(
            registry,
            key,
            "unpublish",
            rule.and_then(|fields| fields.unpublish.as_ref()),
            teams,
        )?,
    })
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
        rules.push(compile_package_rule(registry, ecosystem, key, rule.as_ref(), teams)?);
    }
    Ok(PackageRules::new(rules, default_access))
}
