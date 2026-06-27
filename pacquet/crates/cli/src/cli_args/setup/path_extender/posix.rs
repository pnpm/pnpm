//! Extend the `PATH` (and a proxy variable like `PNPM_HOME`) by editing the
//! current POSIX shell's rc file.
//!
//! Ports pnpm's
//! [`@pnpm/os.env.path-extender-posix`](https://github.com/pnpm/pnpm/blob/1819226b51/packages/path-extender-posix/src/path-extender-posix.ts):
//! the shell is inferred from the environment, the settings block for that
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
    let current_shell = detect_current_shell();
    update_shell(current_shell.as_deref(), dir, opts)
}

/// Mirrors pnpm's `detectCurrentShell`: a shell-specific version variable
/// wins, then the basename of `$SHELL`.
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
fn render_posix_settings(dir: &str, opts: &AddDirToEnvPathOpts) -> String {
    if let Some(proxy) = opts.proxy_var_name {
        let path_ref = match opts.proxy_var_sub_dir {
            Some(sub_dir) => format!("${proxy}/{sub_dir}"),
            None => format!("${proxy}"),
        };
        format!(
            "export {proxy}=\"{dir}\"\ncase \":$PATH:\" in\n  *\":{path_ref}:\"*) ;;\n  *) export PATH=\"{path_value}\" ;;\nesac",
            path_value = create_path_value(opts.position, &path_ref),
        )
    } else {
        format!(
            "case \":$PATH:\" in\n  *\":{dir}:\"*) ;;\n  *) export PATH=\"{path_value}\" ;;\nesac",
            path_value = create_path_value(opts.position, dir),
        )
    }
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
            Some(_) => format!("\"{path_ref}\""),
            None => path_ref.clone(),
        };
        format!(
            "set -gx {proxy} \"{dir}\"\nif not string match -q -- {match_pattern} $PATH\n  set -gx PATH {path_value}\nend",
            path_value = create_fish_path_value(opts.position, &path_ref),
        )
    } else {
        format!(
            "if not string match -q -- \"{dir}\" $PATH\n  set -gx PATH {path_value}\nend",
            path_value = create_fish_path_value(opts.position, dir),
        )
    }
}

fn create_fish_path_value(position: AddingPosition, dir: &str) -> String {
    match position {
        AddingPosition::Start => format!("\"{dir}\" $PATH"),
        AddingPosition::End => format!("$PATH \"{dir}\""),
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
                Some(sub_dir) => format!("($env.{proxy} | path join \"{sub_dir}\")"),
                None => format!("$env.{proxy}"),
            };
            (format!("$env.{proxy} = \"{dir}\"\n"), path_ref)
        }
        None => (String::new(), dir.to_string()),
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
        fs::write(config_file, format!("{new_content}\n"))?;
        return Ok((ConfigFileChangeType::Created, String::new()));
    }
    let config_content = fs::read_to_string(config_file)?;
    let Some((matched_range, old_settings)) =
        find_section(&config_content, opts.config_section_name)
    else {
        fs::write(config_file, format!("{config_content}\n{new_content}\n"))?;
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
        fs::write(config_file, new_config_content)?;
        return Ok((ConfigFileChangeType::Modified, old_settings));
    }
    Ok((ConfigFileChangeType::Skipped, old_settings))
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
