//! Extend the `PATH` (and a proxy variable like `PNPM_HOME`) by editing the
//! current POSIX shell's rc file.
//!
//! The shell is inferred from the environment, the settings block for that
//! shell is rendered, and a `# <section>` ... `# <section> end` block is
//! created in / appended to / replaced in the rc file.

use super::{
    AddDirToEnvPathOpts, AddingPosition, ConfigFileChangeType, ConfigReport, PathExtenderError,
    PathExtenderReport,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(super) fn add_dir_to_posix_env_path(
    dir: &Path,
    opts: &AddDirToEnvPathOpts,
) -> Result<PathExtenderReport, PathExtenderError> {
    // Defense in depth: `handler` validates before any side effect, but
    // re-check here so the renderers never see a `:` that single-quote
    // escaping cannot neutralize (it would still split the colon-delimited
    // `PATH` once `$PNPM_HOME/bin` expands).
    super::validate_posix_pnpm_home(dir)?;
    let current_shell = detect_current_shell();
    update_shell(current_shell.as_deref(), dir, opts)
}

/// A shell-specific version variable wins, then the basename of `$SHELL`.
fn detect_current_shell() -> Option<String> {
    if std::env::var_os("ZSH_VERSION").is_some() {
        return Some("zsh".to_string());
    }
    if std::env::var_os("BASH_VERSION").is_some() {
        return Some("bash".to_string());
    }
    if std::env::var_os("FISH_VERSION").is_some() {
        return Some("fish".to_string());
    }
    if std::env::var_os("NU_VERSION").is_some() {
        return Some("nu".to_string());
    }
    let shell = std::env::var("SHELL").ok()?;
    Path::new(&shell).file_name().map(|name| name.to_string_lossy().into_owned())
}

fn update_shell(
    current_shell: Option<&str>,
    pnpm_home_dir: &Path,
    opts: &AddDirToEnvPathOpts,
) -> Result<PathExtenderReport, PathExtenderError> {
    match current_shell {
        Some("bash" | "zsh" | "ksh" | "dash" | "sh") => {
            // SAFETY of the unwrap: the match guarantees `current_shell` is `Some`.
            setup_shell(current_shell.unwrap(), pnpm_home_dir, opts)
        }
        Some("fish") => setup_fish_shell(pnpm_home_dir, opts),
        Some("nu") => setup_nu_shell(pnpm_home_dir, opts),
        Some(other) => Err(PathExtenderError::UnsupportedShell { shell: other.to_string() }),
        None => Err(PathExtenderError::UnknownShell),
    }
}

fn setup_shell(
    shell: &str,
    dir: &Path,
    opts: &AddDirToEnvPathOpts,
) -> Result<PathExtenderReport, PathExtenderError> {
    let config_file = get_config_file_path(shell)?;
    let new_settings = render_posix_settings(&dir.to_string_lossy(), opts);
    let content = wrap_settings(opts.config_section_name, &new_settings);
    let (change_type, old_settings) = update_shell_config(&config_file, &content, opts)?;
    Ok(PathExtenderReport {
        config_file: Some(ConfigReport { path: config_file, change_type }),
        old_settings,
        new_settings,
    })
}

/// The `# <section>` body for a POSIX `sh`-family shell. Pure so the
/// rendering can be unit-tested without touching the filesystem.
///
/// `dir` is single-quote escaped before it is interpolated into the shell
/// code. This hardens beyond pnpm's `@pnpm/os.env.path-extender`, which
/// interpolates the directory into double quotes — where a value containing
/// `$(...)` / backticks would execute when the rc file is sourced.
fn render_posix_settings(dir: &str, opts: &AddDirToEnvPathOpts) -> String {
    if let Some(proxy) = opts.proxy_var_name {
        let path_ref = match opts.proxy_var_sub_dir {
            Some(sub_dir) => format!("${proxy}/{sub_dir}"),
            None => format!("${proxy}"),
        };
        format!(
            "export {proxy}={value}\ncase \":$PATH:\" in\n  *\":{path_ref}:\"*) ;;\n  *) export PATH=\"{path_value}\" ;;\nesac",
            value = sh_quote(dir),
            path_value = create_path_value(opts.position, &path_ref),
        )
    } else {
        let quoted = sh_quote(dir);
        format!(
            "case \":$PATH:\" in\n  *\":\"{quoted}\":\"*) ;;\n  *) export PATH={path_value} ;;\nesac",
            path_value = create_path_value(opts.position, &quoted),
        )
    }
}

/// Wrap `value` in single quotes, escaping any embedded single quote as
/// `'\''`, so the POSIX shell treats it as a literal with no expansion.
fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

fn create_path_value(position: AddingPosition, dir: &str) -> String {
    match position {
        AddingPosition::Start => format!("{dir}:$PATH"),
        AddingPosition::End => format!("$PATH:{dir}"),
    }
}

fn get_config_file_path(shell: &str) -> Result<PathBuf, PathExtenderError> {
    match shell {
        "zsh" => Ok(zdotdir_or_home()?.join(".zshrc")),
        "dash" | "sh" => match std::env::var("ENV").ok().filter(|env| !env.is_empty()) {
            Some(env) => Ok(PathBuf::from(env)),
            None => Err(PathExtenderError::NoShellConfig { shell: shell.to_string() }),
        },
        _ => Ok(home_dir()?.join(format!(".{shell}rc"))),
    }
}

fn setup_fish_shell(
    dir: &Path,
    opts: &AddDirToEnvPathOpts,
) -> Result<PathExtenderReport, PathExtenderError> {
    let config_file = home_dir()?.join(".config/fish/config.fish");
    let new_settings = render_fish_settings(&dir.to_string_lossy(), opts);
    let content = wrap_settings(opts.config_section_name, &new_settings);
    let (change_type, old_settings) = update_shell_config(&config_file, &content, opts)?;
    Ok(PathExtenderReport {
        config_file: Some(ConfigReport { path: config_file, change_type }),
        old_settings,
        new_settings,
    })
}

fn render_fish_settings(dir: &str, opts: &AddDirToEnvPathOpts) -> String {
    if let Some(proxy) = opts.proxy_var_name {
        let path_ref = match opts.proxy_var_sub_dir {
            Some(sub_dir) => format!("${proxy}/{sub_dir}"),
            None => format!("${proxy}"),
        };
        let match_pattern = match opts.proxy_var_sub_dir {
            Some(_) => format!(r#""{path_ref}""#),
            None => path_ref.clone(),
        };
        format!(
            "set -gx {proxy} {value}\nif not string match -q -- {match_pattern} $PATH\n  set -gx PATH {path_value}\nend",
            value = fish_quote(dir),
            path_value = create_fish_path_value(opts.position, &format!(r#""{path_ref}""#)),
        )
    } else {
        let quoted = fish_quote(dir);
        format!(
            "if not string match -q -- {quoted} $PATH\n  set -gx PATH {path_value}\nend",
            path_value = create_fish_path_value(opts.position, &quoted),
        )
    }
}

/// Wrap `value` in fish single quotes, escaping `\` and `'` (the only two
/// characters fish recognizes inside single quotes), so fish treats it as a
/// literal with no expansion.
fn fish_quote(value: &str) -> String {
    format!("'{}'", value.replace('\\', r"\\").replace('\'', r"\'"))
}

/// Build the fish `PATH` list value from a pre-quoted `entry` (a
/// double-quoted variable reference for the proxy case, or a single-quoted
/// literal directory for the no-proxy case).
fn create_fish_path_value(position: AddingPosition, entry: &str) -> String {
    match position {
        AddingPosition::Start => format!("{entry} $PATH"),
        AddingPosition::End => format!("$PATH {entry}"),
    }
}

fn setup_nu_shell(
    dir: &Path,
    opts: &AddDirToEnvPathOpts,
) -> Result<PathExtenderReport, PathExtenderError> {
    let config_file = home_dir()?.join(".config/nushell/env.nu");
    let new_settings = render_nu_settings(&dir.to_string_lossy(), opts);
    let content = wrap_settings(opts.config_section_name, &new_settings);
    let (change_type, old_settings) = update_shell_config(&config_file, &content, opts)?;
    Ok(PathExtenderReport {
        config_file: Some(ConfigReport { path: config_file, change_type }),
        old_settings,
        new_settings,
    })
}

fn render_nu_settings(dir: &str, opts: &AddDirToEnvPathOpts) -> String {
    let adding_command = match opts.position {
        AddingPosition::Start => "prepend",
        AddingPosition::End => "append",
    };
    let (prefix, path_ref) = match opts.proxy_var_name {
        Some(proxy) => {
            let path_ref = match opts.proxy_var_sub_dir {
                Some(sub_dir) => format!(r#"($env.{proxy} | path join "{sub_dir}")"#),
                None => format!("$env.{proxy}"),
            };
            (format!("$env.{proxy} = {value}\n", value = nu_quote(dir)), path_ref)
        }
        None => (String::new(), nu_quote(dir)),
    };
    // Built piecewise rather than with one long `format!`: the PATH line
    // exceeds the line-width limit, which would force a multi-line macro
    // invocation that conflicts with the no-trailing-comma rule for
    // argument-less `format!`s.
    let mut settings = prefix;
    settings.push_str("$env.PATH = ($env.PATH | split row (char esep) | ");
    settings.push_str(adding_command);
    settings.push(' ');
    settings.push_str(&path_ref);
    settings.push_str(" )");
    settings
}

/// Wrap `value` in nushell double quotes, escaping `\` and `"`. nushell does
/// not perform command substitution or variable interpolation inside a plain
/// double-quoted string (only `$"..."` interpolates), so the result is a
/// literal with no expansion.
fn nu_quote(value: &str) -> String {
    format!(r#""{}""#, value.replace('\\', r"\\").replace('"', r#"\""#))
}

fn wrap_settings(section_name: &str, settings: &str) -> String {
    format!("# {section_name}\n{settings}\n# {section_name} end")
}

fn update_shell_config(
    config_file: &Path,
    new_content: &str,
    opts: &AddDirToEnvPathOpts,
) -> Result<(ConfigFileChangeType, String), PathExtenderError> {
    if !config_file.exists() {
        if let Some(parent) = config_file.parent() {
            fs::create_dir_all(parent)?;
        }
        write_config(config_file, &format!("{new_content}\n"))?;
        return Ok((ConfigFileChangeType::Created, String::new()));
    }
    let config_content = fs::read_to_string(config_file)?;
    let Some((matched_range, old_settings)) =
        find_section(&config_content, opts.config_section_name)
    else {
        write_config(config_file, &format!("{config_content}\n{new_content}\n"))?;
        return Ok((ConfigFileChangeType::Appended, String::new()));
    };
    if &config_content[matched_range] != new_content {
        if !opts.overwrite {
            return Err(PathExtenderError::BadShellSection {
                config_file: config_file.to_path_buf(),
                config_section_name: opts.config_section_name.to_string(),
            });
        }
        let new_config_content =
            replace_section(&config_content, new_content, opts.config_section_name);
        write_config(config_file, &new_config_content)?;
        return Ok((ConfigFileChangeType::Modified, old_settings));
    }
    Ok((ConfigFileChangeType::Skipped, old_settings))
}

/// Overwrite the rc file crash-safely via [`pacquet_fs::ensure_file`], the
/// repo's hardened atomic writer: it writes through a unique sibling temp
/// file opened with `O_CREAT|O_EXCL` (so it never follows a pre-seeded
/// symlink or truncates an attacker-planted path) and renames it over the
/// target.
fn write_config(path: &Path, content: &str) -> Result<(), PathExtenderError> {
    pacquet_fs::ensure_file(path, content.as_bytes(), None)?;
    Ok(())
}

/// Locate the `# <section>` ... `# <section> end` block, returning the byte
/// range of the whole block and the inner settings between the markers.
/// Mirrors pnpm's greedy `# <section>\n([\s\S]*)\n# <section> end` match:
/// the block opens at the first `# <section>\n` and closes at the last
/// `\n# <section> end`.
fn find_section(content: &str, section: &str) -> Option<(std::ops::Range<usize>, String)> {
    let start_pat = format!("# {section}\n");
    let end_pat = format!("\n# {section} end");
    let start = content.find(&start_pat)?;
    let inner_start = start + start_pat.len();
    let end = content.rfind(&end_pat)?;
    if end < inner_start {
        return None;
    }
    let inner = content[inner_start..end].to_string();
    Some((start..end + end_pat.len(), inner))
}

/// Replace the `# <section>` ... `# <section> end` block with `new_section`.
/// Mirrors pnpm's greedy `# <section>[\s\S]*# <section> end` replacement.
fn replace_section(content: &str, new_section: &str, section: &str) -> String {
    let begin_pat = format!("# {section}");
    let end_pat = format!("# {section} end");
    let begin = content.find(&begin_pat).unwrap_or(0);
    let end = content.rfind(&end_pat).map_or(content.len(), |index| index + end_pat.len());
    format!("{}{}{}", &content[..begin], new_section, &content[end..])
}

fn home_dir() -> Result<PathBuf, PathExtenderError> {
    home::home_dir().ok_or(PathExtenderError::NoHomeDir)
}

fn zdotdir_or_home() -> Result<PathBuf, PathExtenderError> {
    match std::env::var("ZDOTDIR").ok().filter(|dir| !dir.is_empty()) {
        Some(dir) => Ok(PathBuf::from(dir)),
        None => home_dir(),
    }
}

#[cfg(test)]
mod tests;
