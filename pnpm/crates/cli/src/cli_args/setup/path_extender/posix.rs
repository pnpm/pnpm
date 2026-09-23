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
    Path::new(&shell)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
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
        "dash" | "sh" => match std::env::var("ENV")
            .ok()
            .filter(|env| !env.is_empty())
        {
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
    if config_content[matched_range].replace("\r\n", "\n") != new_content {
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

/// Overwrite the rc file crash-safely via [`pnpm_fs::ensure_file`], the
/// repo's hardened atomic writer: it writes through a unique sibling temp
/// file opened with `O_CREAT|O_EXCL` (so it never follows a pre-seeded
/// symlink or truncates an attacker-planted path) and renames it over the
/// target.
fn write_config(path: &Path, content: &str) -> Result<(), PathExtenderError> {
    pnpm_fs::ensure_file(path, content.as_bytes(), None)?;
    Ok(())
}

fn complete_section(
    content: &str,
    start_offset: usize,
    inner_start: usize,
    line_start: usize,
    line: &str,
) -> (std::ops::Range<usize>, String) {
    let inner_len = content[inner_start..line_start]
        .trim_end_matches(['\r', '\n'])
        .len();
    let inner = content[inner_start..inner_start + inner_len].to_string();
    let marker_len = line
        .trim_end_matches(['\r', '\n'])
        .len();
    (start_offset..line_start + marker_len, inner)
}

fn parse_sections(content: &str, section: &str) -> Vec<(std::ops::Range<usize>, String)> {
    let start_marker = format!("# {section}");
    let end_marker = format!("# {section} end");
    let mut sections = Vec::new();
    let mut last_start = None;
    let mut offset = 0;

    for line in content.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        let trimmed = line.trim_end_matches(['\r', '\n', ' ', '\t']);

        if trimmed == start_marker {
            last_start = Some((line_start, offset));
        } else if trimmed == end_marker
            && let Some((start, inner)) = last_start.take()
        {
            sections.push(complete_section(content, start, inner, line_start, line));
        }
    }

    sections
}

fn select_section(
    mut sections: Vec<(std::ops::Range<usize>, String)>,
    section: &str,
) -> Option<(std::ops::Range<usize>, String)> {
    if sections.len() <= 1 {
        return sections.pop();
    }
    let home_var = format!("{}_HOME", section.to_uppercase());
    let settings: Vec<String> = sections
        .iter()
        .map(|(_, inner)| strip_comments(inner))
        .collect();
    let predicates: [&dyn Fn(&str) -> bool; 3] = [
        &|text| text.contains("PATH") && text.contains(&home_var),
        &|text| text.contains(&home_var),
        &|text| text.contains("PATH"),
    ];
    for predicate in predicates {
        if let Some(idx) = settings.iter().rposition(|text| predicate(text)) {
            return Some(sections.swap_remove(idx));
        }
    }
    sections.pop()
}

fn strip_comments(settings: &str) -> String {
    settings
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Find a `# <section>` ... `# <section> end` section and return its full byte range
/// along with the inner configuration text.
///
/// A valid section is bounded by an opening `# <section>` line and a closing
/// `# <section> end` line with no intermediate `# <section>` or `# <section> end`
/// markers. If several valid sections exist, the last one whose non-comment
/// lines reference both `PATH` and `<SECTION>_HOME` wins, then the last one
/// referencing `<SECTION>_HOME`, then the last one referencing `PATH`, then
/// the last section.
fn find_section(content: &str, section: &str) -> Option<(std::ops::Range<usize>, String)> {
    if content.is_empty() {
        return None;
    }
    let sections = parse_sections(content, section);
    select_section(sections, section)
}

/// Replace the `# <section>` ... `# <section> end` block with `new_section`.
fn replace_section(content: &str, new_section: &str, section: &str) -> String {
    if let Some((range, _)) = find_section(content, section) {
        format!("{}{}{}", &content[..range.start], new_section, &content[range.end..])
    } else {
        content.to_string()
    }
}

fn home_dir() -> Result<PathBuf, PathExtenderError> {
    home::home_dir().ok_or(PathExtenderError::NoHomeDir)
}

fn zdotdir_or_home() -> Result<PathBuf, PathExtenderError> {
    match std::env::var("ZDOTDIR")
        .ok()
        .filter(|dir| !dir.is_empty())
    {
        Some(dir) => Ok(PathBuf::from(dir)),
        None => home_dir(),
    }
}

#[cfg(test)]
mod tests;
