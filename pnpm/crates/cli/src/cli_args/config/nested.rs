use super::{
    Config,
    ConfigError,
    ConfigFlags,
    Path,
    Segment,
    Value,
    YamlConfigTarget,
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
    let mut keys: Vec<_> = segments
        .iter()
        .map(|segment| match segment {
            Segment::Key(key) => Ok(key.as_str()),
            Segment::Index(_) => Err(ConfigError::SetDeepKey),
        })
        .collect::<Result<_, _>>()?;
    let (root, fields) = keys.split_first_mut().expect("a nested path has a root");
    let global = resolve_global(flags);
    let scope = if global { YamlConfigTarget::Global } else { YamlConfigTarget::Project };
    if config_types::is_ini_config_key(root)
        || config_types::is_ini_config_key(&naming_cases::to_kebab_case(root))
    {
        return Err(ConfigError::SetDeepKey.into());
    }
    let key = validate_yaml_target_key(root, scope)?;
    let (dir, _) = get_config_file_info(&key, global, config, dir)?;
    let normalized;
    if key == "globalPackagesConfig" && pnpm_config::known_settings::is_known_setting_key(fields[0])
    {
        normalized = naming_cases::to_camel_case(fields[0]);
        fields[0] = &normalized;
    }
    update_nested_setting(&dir, scope, &key, fields, value, !flags.json)
}
