use super::{
    ManifestTarget,
    WorkspaceManifests,
    field_updates::{
        WritePrivacy,
        write_target_text,
    },
    read_if_present,
};
use miette::{
    Context,
    IntoDiagnostic,
};
use pnpm_config::{
    GLOBAL_CONFIG_YAML_FILENAME,
    YamlConfigTarget,
    protected_settings::carries_global_credentials,
    validate_nested_setting,
};
use pnpm_workspace::{
    SettingSlot,
    WorkspaceIncludes,
};
use serde_json::{
    Map,
    Value,
};
use std::path::Path;

pub enum SettingInput {
    Plain(String),
    Json(Value),
}

impl From<SettingInput> for Value {
    fn from(input: SettingInput) -> Self {
        match input {
            SettingInput::Plain(text) => Self::String(text),
            SettingInput::Json(value) => value,
        }
    }
}

impl SettingInput {
    fn candidates(self, key: &str, fields: &[&str]) -> (Value, Option<Value>) {
        let text = match self {
            Self::Plain(text) => text,
            Self::Json(value) => return (value, None),
        };
        let raw = text.trim();
        if let Ok(boolean) = raw.parse::<bool>() {
            if is_build_decision(key, fields) {
                return (Value::Bool(boolean), None);
            }
            return (Value::String(text), Some(Value::Bool(boolean)));
        }
        let number = raw
            .parse::<serde_json::Number>()
            .ok()
            .map(Value::Number);
        (Value::String(text), number)
    }
}

fn is_build_decision(root: &str, fields: &[&str]) -> bool {
    let (key, path) = match (root, fields) {
        ("globalPackagesConfig", [setting, rest @ ..]) => (*setting, rest),
        path => path,
    };
    matches!(
        (key, path),
        ("allowBuilds", [_]) | ("permissions", [_, "build"] | ["rules", _, "build"]),
    )
}

/// Set a field in the raw YAML mapping owned by this scope, preserving sibling entries.
pub fn update_nested_setting(
    dir: &Path,
    scope: YamlConfigTarget,
    key: &str,
    fields: &[&str],
    input: SettingInput,
) -> miette::Result<()> {
    let (target, includes) = setting_target(dir, scope, key, fields)?;
    let text = read_if_present(&target.path)?.unwrap_or_default();
    let mut document = pnpm_yaml_document_sync::parse(&text)
        .into_diagnostic()
        .wrap_err_with(|| format!("reading {}", target.path.display()))?;
    if document.is_null() {
        document = Value::Object(Map::new());
    }
    let (value, fallback) = input.candidates(key, fields);
    let value =
        if target.included { rebase_field(&target, dir, key, fields, value)? } else { value };
    if !set_field(&mut document, key, fields, value)? {
        return Ok(());
    }
    validate_section(&mut document, key, fields, scope, fallback)?;
    let section = &document[key];
    if let Some(includes) = includes {
        includes.check_setting_write(
            &target.path,
            key,
            &yaml_serde::to_value(section).into_diagnostic()?,
        )?;
    }
    let privacy = if scope == YamlConfigTarget::Global && carries_global_credentials(key, section) {
        WritePrivacy::OwnerOnly
    } else {
        WritePrivacy::InheritOrUmask
    };
    let updated = pnpm_yaml_document_sync::sync(&text, &document)
        .into_diagnostic()
        .wrap_err_with(|| format!("editing {}", target.path.display()))?;
    if updated != text || matches!(privacy, WritePrivacy::OwnerOnly) {
        write_target_text(&target.path, true, &updated, privacy)?;
    }
    Ok(())
}

fn validate_section(
    document: &mut Value,
    key: &str,
    fields: &[&str],
    scope: YamlConfigTarget,
    fallback: Option<Value>,
) -> miette::Result<()> {
    if matches!(key, "catalog" | "catalogs")
        && document.get("catalog").is_some()
        && document.pointer("/catalogs/default").is_some()
    {
        miette::bail!(
            "The default catalog already exists. Set the field under its existing YAML key.",
        );
    }
    if let Err(reason) = validate_nested_setting(key, &document[key], scope) {
        let error = miette::miette!("Invalid value for {key:?}: {reason}");
        let Some(scalar) = fallback else {
            return Err(error);
        };
        let field = fields
            .iter()
            .try_fold(&mut document[key], |node, name| node.get_mut(*name))
            .expect("set_field inserted this field");
        *field = scalar;
        validate_nested_setting(key, &document[key], scope).map_err(|_| error)?;
    }
    Ok(())
}

fn setting_target(
    dir: &Path,
    scope: YamlConfigTarget,
    key: &str,
    fields: &[&str],
) -> miette::Result<(ManifestTarget, Option<WorkspaceIncludes>)> {
    let Some((first, _)) = fields.split_first() else {
        miette::bail!("Expected a field of {key:?}");
    };
    Ok(match scope {
        YamlConfigTarget::Global => {
            (ManifestTarget::root_manifest(&dir.join(GLOBAL_CONFIG_YAML_FILENAME)), None)
        }
        YamlConfigTarget::Project => {
            let manifests = WorkspaceManifests::read(dir)?;
            let target = if key == "catalogs" {
                manifests.target_for(&SettingSlot::Catalog((*first).to_owned()))?
            } else {
                manifests.target_for_setting(key)?
            };
            (target, manifests.includes)
        }
    })
}

fn rebase_field(
    target: &ManifestTarget,
    dir: &Path,
    key: &str,
    fields: &[&str],
    value: Value,
) -> miette::Result<Value> {
    let wrapped = fields
        .iter()
        .rev()
        .fold(value, |inner, name| Value::Object(Map::from_iter([((*name).to_owned(), inner)])));
    let mut rebased = target.setting_from(dir, key, &wrapped);
    fields
        .iter()
        .try_fold(&mut rebased, |node, name| node.get_mut(*name))
        .map(Value::take)
        .ok_or_else(|| miette::miette!("Cannot rebase a field of {key:?}"))
}

fn set_field(
    document: &mut Value,
    key: &str,
    fields: &[&str],
    value: Value,
) -> miette::Result<bool> {
    let (field, parents) = fields.split_last().expect("the caller checked for a field");
    let mut parent = document;
    for name in std::iter::once(key).chain(parents.iter().copied()) {
        let mapping = as_mapping(parent, name)?;
        if value.is_null() && !mapping.contains_key(name) {
            return Ok(false);
        }
        parent = mapping
            .entry(name)
            .or_insert_with(|| Value::Object(Map::new()));
    }
    let mapping = as_mapping(parent, field)?;
    if value.is_null() {
        return Ok(mapping.remove(*field).is_some());
    }
    mapping.insert((*field).to_owned(), value);
    Ok(true)
}

fn as_mapping<'a>(node: &'a mut Value, field: &str) -> miette::Result<&'a mut Map<String, Value>> {
    node.as_object_mut()
        .ok_or_else(|| miette::miette!("Cannot set {field:?}: its parent must be a mapping"))
}
