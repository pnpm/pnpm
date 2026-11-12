//! PowerShell on a non-Windows platform: the profile it loads and the block
//! `setup` writes to it.

use super::{
    AddDirToEnvPathOpts, AddingPosition, ConfigReport, PathExtenderError, PathExtenderReport,
    home_dir, update_shell_config, wrap_section,
};
use std::path::{Path, PathBuf};

pub(super) fn setup_pwsh_shell(
    dir: &Path,
    opts: &AddDirToEnvPathOpts,
) -> Result<PathExtenderReport, PathExtenderError> {
    let config_file = xdg_config_home()?.join("powershell/profile.ps1");
    let new_settings = render_pwsh_settings(&dir.to_string_lossy(), opts);
    let content = wrap_section(opts.config_section_name, &new_settings);
    let (change_type, old_settings) = update_shell_config(&config_file, &content, &[], opts)?;
    Ok(PathExtenderReport {
        config_file: Some(ConfigReport { path: config_file, change_type }),
        old_settings,
        new_settings,
    })
}

/// The block for PowerShell on a non-Windows platform, where `PATH` is
/// `:`-separated. `-cne` compares case-sensitively, as the file system does.
pub(super) fn render_pwsh_settings(dir: &str, opts: &AddDirToEnvPathOpts) -> String {
    let (prefix, entry) = match opts.proxy_var_name {
        Some(proxy) => {
            let path_ref = match opts.proxy_var_sub_dir {
                Some(sub_dir) => format!(r#""$env:{proxy}/{sub_dir}""#),
                None => format!("$env:{proxy}"),
            };
            (format!("$env:{proxy} = {}\n", pwsh_quote(dir)), path_ref)
        }
        None => (String::new(), pwsh_quote(dir)),
    };
    let (index, path_value) = match opts.position {
        AddingPosition::Start => ("0", format!("{entry} + ':' + $env:PATH")),
        AddingPosition::End => ("-1", format!("$env:PATH + ':' + {entry}")),
    };
    format!(
        "{prefix}if (($env:PATH -split ':')[{index}] -cne {entry}) {{\n  $env:PATH = {path_value}\n}}",
    )
}

/// Wrap `value` in PowerShell single quotes, doubling every quote character,
/// so PowerShell treats it as a literal with no expansion. PowerShell also
/// treats typographic apostrophes as string delimiters.
pub(in crate::cli_args::setup) fn pwsh_quote(value: &str) -> String {
    let mut quoted = String::from('\'');
    for character in value.chars() {
        quoted.push(character);
        if matches!(character, '\'' | '\u{2018}'..='\u{201b}') {
            quoted.push(character);
        }
    }
    quoted.push('\'');
    quoted
}

/// The directory PowerShell reads its profile from: `$XDG_CONFIG_HOME`, else
/// `~/.config`.
fn xdg_config_home() -> Result<PathBuf, PathExtenderError> {
    match std::env::var_os("XDG_CONFIG_HOME").filter(|dir| !dir.is_empty()) {
        Some(dir) => Ok(PathBuf::from(dir)),
        None => Ok(home_dir()?.join(".config")),
    }
}
