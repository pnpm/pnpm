use super::{
    Context, GlobalError, GlobalPackageInfo, HashMap, IntoDiagnostic, Path, PathBuf, fs,
    is_plain_version_spec, is_valid_old_npm_package_name, lexical_normalize,
    parse_wanted_dependency, safe_read_package_json_from_dir, tool_install_selector,
};

/// A tool name becomes the selector that installs the tool itself, which
/// the ordinary pipeline then handles — so the result stays a normal global
/// install that `pnpm ls -g` and `pnpm remove -g` see.
pub(super) fn tool_install_selectors(groups: Vec<Vec<String>>) -> Vec<Vec<String>> {
    groups
        .into_iter()
        .map(|group| {
            group.into_iter().map(|token| tool_install_selector(&token).unwrap_or(token)).collect()
        })
        .collect()
}

/// The installed groups the update targets. `None` when the command
/// named packages that are not installed, which it reports and treats as
/// a no-op.
pub(super) fn groups_matching_params(
    all: Vec<GlobalPackageInfo>,
    params: &[String],
) -> Option<Vec<GlobalPackageInfo>> {
    if params.is_empty() {
        return Some(all);
    }
    let filtered: Vec<GlobalPackageInfo> =
        all.into_iter().filter(|pkg| params.iter().any(|param| pkg.has_alias(param))).collect();
    if filtered.is_empty() {
        println!("No matching global packages found");
        return None;
    }
    Some(filtered)
}

/// With `--latest`, a dependency is reduced to its bare alias so the newest
/// registry version is resolved.
/// The selectors that reinstall a group. With `--latest` a plain version spec
/// is dropped so the newest release is picked; `pins` holds back the aliases
/// that would otherwise move backwards.
pub(super) fn update_selectors(
    dependencies: &[(String, String)],
    latest: bool,
    pins: &HashMap<String, String>,
) -> Vec<String> {
    dependencies
        .iter()
        .map(|(alias, spec)| {
            if let Some(pin) = pins.get(alias) {
                format!("{alias}@{pin}")
            } else if latest && is_plain_version_spec(spec) {
                alias.clone()
            } else {
                format!("{alias}@{spec}")
            }
        })
        .collect()
}

pub(super) fn replacement_aliases(aliases: &[String]) -> Vec<String> {
    const PNPM_CLI_PACKAGE_ALIASES: [&str; 2] = ["pnpm", "@pnpm/exe"];

    let mut expanded = aliases.to_vec();
    if aliases.iter().any(|alias| is_pnpm_cli_package_name(alias)) {
        for alias in PNPM_CLI_PACKAGE_ALIASES {
            if !expanded.iter().any(|existing| existing == alias) {
                expanded.push(alias.to_string());
            }
        }
    }
    expanded
}

pub(super) fn should_replace_existing_package(
    pkg: &GlobalPackageInfo,
    aliases: &[String],
    aliases_to_replace: &[String],
) -> bool {
    if aliases.iter().any(|alias| pkg.has_alias(alias)) {
        return true;
    }
    is_pnpm_cli_only_group(pkg) && aliases_to_replace.iter().any(|alias| pkg.has_alias(alias))
}

/// Whether `pkg` is a global group the pnpm CLI is installed in — the install
/// that `pnpm self-update` owns. `update -g` leaves the whole group alone:
/// reinstalling it would relink pnpm's bin whatever else the group holds.
pub fn has_pnpm_cli_dependency(pkg: &GlobalPackageInfo) -> bool {
    pkg.dependencies.iter().any(|(alias, spec)| is_pnpm_cli_dependency(alias, Some(spec)))
}

/// Whether `pkg` is a global group holding nothing but the pnpm CLI. `add -g`
/// refuses to create one.
fn is_pnpm_cli_only_group(pkg: &GlobalPackageInfo) -> bool {
    !pkg.dependencies.is_empty()
        && pkg.dependencies.iter().all(|(alias, spec)| is_pnpm_cli_dependency(alias, Some(spec)))
}

/// Whether any of `params` names the pnpm CLI itself. Each selector is
/// normalized to the package it installs first, so neither a versioned form
/// like `pnpm@9` nor an aliased one like `foo@npm:pnpm@9` bypasses the guard.
pub fn selects_pnpm_cli<'a>(params: impl IntoIterator<Item = &'a String>) -> bool {
    params.into_iter().any(|param| {
        let parsed = parse_wanted_dependency(param);
        is_pnpm_cli_dependency(
            parsed.alias.as_deref().unwrap_or_default(),
            parsed.bare_specifier.as_deref(),
        )
    })
}

/// Whether a dependency declared as `alias` at `spec` is the pnpm CLI. An
/// `npm:` alias resolves to its target, so `foo` at `npm:pnpm@9` is the pnpm
/// CLI under another name — the install still carries pnpm's own `pnpm` bin.
fn is_pnpm_cli_dependency(alias: &str, spec: Option<&str>) -> bool {
    let name = npm_alias_target(spec);
    is_pnpm_cli_package_name(name.as_deref().unwrap_or(alias))
}

fn is_pnpm_cli_package_name(name: &str) -> bool {
    matches!(name, "pnpm" | "@pnpm/exe")
}

/// The package an `npm:` alias points at, or `None` for any other spec.
fn npm_alias_target(spec: Option<&str>) -> Option<String> {
    parse_wanted_dependency(spec?.strip_prefix("npm:")?).alias
}

// --- param grouping (split/resolve helpers) -------------------------------

pub(super) fn split_into_groups(params: &[String], base_dir: &Path) -> Vec<Vec<String>> {
    params
        .iter()
        .map(|param| {
            split_comma_separated(param, base_dir)
                .into_iter()
                .map(|token| resolve_local_param(&token, base_dir))
                .collect::<Vec<String>>()
        })
        .filter(|group| !group.is_empty())
        .collect()
}

pub(super) fn split_comma_separated(param: &str, base_dir: &Path) -> Vec<String> {
    if !param.contains(',') {
        return vec![param.to_string()];
    }
    if param.contains("://") {
        return vec![param.to_string()];
    }
    if refers_to_existing_local_path(param, base_dir) {
        return vec![param.to_string()];
    }
    param.split(',').map(str::trim).filter(|token| !token.is_empty()).map(str::to_string).collect()
}

fn refers_to_existing_local_path(param: &str, base_dir: &Path) -> bool {
    let path_part = if let Some(rest) = param.strip_prefix("file:") {
        rest
    } else if let Some(rest) = param.strip_prefix("link:") {
        rest
    } else if param.starts_with('.')
        || param.starts_with('/')
        || param.starts_with('~')
        || is_windows_drive_path(param)
    {
        param
    } else {
        return false;
    };
    let resolved = if Path::new(path_part).is_absolute() {
        PathBuf::from(path_part)
    } else {
        base_dir.join(path_part)
    };
    resolved.exists()
}

/// Mirror the TypeScript `resolveLocalParam`: rewrite only *dot-relative*
/// `file:`/`link:` selectors against `base_dir`. Bare names, home-relative
/// (`~/`), and absolute selectors pass through unchanged so the local
/// resolver's own `~` expansion and registry fallbacks still apply.
pub(super) fn resolve_local_param(param: &str, base_dir: &Path) -> String {
    for prefix in ["file:", "link:"] {
        if let Some(rest) = param.strip_prefix(prefix) {
            if rest.starts_with('.') {
                return format!("{prefix}{}", lexical_normalize(&base_dir.join(rest)).display());
            }
            return param.to_string();
        }
    }
    if param.starts_with('.') {
        return lexical_normalize(&base_dir.join(param)).display().to_string();
    }
    param.to_string()
}

pub(super) fn infer_local_package_alias(selector: &str) -> miette::Result<String> {
    let Some(path) = selector.strip_prefix("file:").map(Path::new) else {
        return Ok(selector.to_string());
    };
    let path_display = path.display().to_string();
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(selector.to_string());
        }
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err(format!("read local package metadata from {path_display}"));
        }
    };
    if !metadata.is_dir() {
        return Ok(selector.to_string());
    }
    let manifest = safe_read_package_json_from_dir(path)
        .map_err(miette::Report::new)
        .wrap_err_with(|| format!("read local package manifest from {path_display}"))?
        .ok_or_else(|| miette::miette!("No package.json was found in {path_display}"))?;
    let name = manifest
        .get("name")
        .and_then(serde_json::Value::as_str)
        .filter(|name| !name.is_empty())
        .or_else(|| path.file_name().and_then(|name| name.to_str()))
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            miette::miette!("The local package at {path_display} has no package name")
        })?;
    if !is_valid_old_npm_package_name(name) {
        return Err(GlobalError::InvalidPackageName { name: name.to_string() }.into());
    }
    Ok(format!("{name}@{selector}"))
}

pub(super) fn is_windows_drive_path(param: &str) -> bool {
    let bytes = param.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\')
}
