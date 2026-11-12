//! PowerShell on a non-Windows platform: the profile it loads and the block
//! `setup` writes to it.

use super::{
    AddDirToEnvPathOpts, AddingPosition, ConfigReport, PathExtenderError, PathExtenderReport,
    update_shell_config, wrap_section,
};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

/// Write the block to `config_file`, the path [`profile_path`] resolves.
pub(super) fn setup_pwsh_shell(
    config_file: PathBuf,
    dir: &Path,
    opts: &AddDirToEnvPathOpts,
) -> Result<PathExtenderReport, PathExtenderError> {
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

/// The profile pwsh loads for every host: `profile.ps1` under
/// `$XDG_CONFIG_HOME/powershell`, else under `~/.config/powershell`.
pub(super) fn profile_path(
    xdg_config_home: Option<OsString>,
    home_dir: Option<PathBuf>,
) -> Result<PathBuf, PathExtenderError> {
    let config_home = match xdg_config_home.filter(|dir| !dir.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => home_dir.ok_or(PathExtenderError::NoHomeDir)?.join(".config"),
    };
    Ok(config_home.join("powershell/profile.ps1"))
}
