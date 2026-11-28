use super::{
    Config,
    ConfigError,
    ConfigFlags,
    Path,
    Segment,
    Value,
    YamlConfigTarget,
    cast_field,
    config_types,
    get_config_file_info,
    naming_cases,
    resolve_global,
    validate_yaml_target_key,
};
use pnpm_workspace_manifest_writer::update_nested_setting;

pub(super) fn set(
    config: &Config,
    dir: &Path,
    flags: ConfigFlags,
    segments: &[Segment],
    value: Value,
) -> miette::Result<()> {
    if let Some(key) = pnpm_config::property_path::unsafe_key(segments) {
        miette::bail!("Key {key:?} is not allowed in a property path");
    }
    let keys: Vec<_> = segments
        .iter()
        .map(|segment| match segment {
            Segment::Key(key) => Ok(key.as_str()),
            Segment::Index(_) => Err(ConfigError::SetDeepKey),
        })
        .collect::<Result<_, _>>()?;
    let (root, fields) = keys.split_first().expect("a nested path has a root");
    let global = resolve_global(flags);
    let scope = if global { YamlConfigTarget::Global } else { YamlConfigTarget::Project };
    if config_types::is_ini_config_key(root)
        || config_types::is_ini_config_key(&naming_cases::to_kebab_case(root))
    {
        return Err(ConfigError::SetDeepKey.into());
    }
    let key = validate_yaml_target_key(root, scope)?;
    let (dir, _) = get_config_file_info(&key, global, config, dir)?;
    let field = fields.last().expect("a nested path has a field");
    let value =
        if flags.json { value } else { cast_field(value, &naming_cases::to_kebab_case(field)) };
    update_nested_setting(&dir, scope, &key, fields, value)
}
