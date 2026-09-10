use super::{NodePathEnvVar, Path, ScriptRuntime, normalize_node_path_env_var, relative_target};

/// Generate the cross-shell PowerShell `.ps1` shim contents for
/// `target_path`, minus the `prependToPath`/`nodeExecPath`/`progArgs`
/// branches we don't use. `node_path` entries (empty for a plain shim)
/// become the cmd-shim `NODE_PATH` set/restore blocks. The shim
/// self-detects Windows vs. POSIX-ish pwsh and adjusts the executable
/// suffix (and `NODE_PATH` flavor) accordingly.
#[must_use]
pub fn generate_pwsh_shim(
    target_path: &Path,
    shim_path: &Path,
    runtime: Option<&ScriptRuntime>,
    node_path: &[String],
) -> String {
    let quoted_target = quoted_pwsh_target(target_path, shim_path);

    use std::fmt::Write;
    let node_path_header = pwsh_node_path_header(node_path);
    let restore_node_path = node_path_header.is_some().then_some("$env:NODE_PATH=$env_node_path");
    let mut pwsh = node_path_header.unwrap_or_else(|| String::from(PWSH_SHIM_HEADER));

    match runtime {
        Some(ScriptRuntime { prog: Some(prog), args }) => {
            let long_prog = format!(r#""$basedir/{prog}$exe""#);
            let prog_quoted = format!(r#""{prog}$exe""#);
            writeln!(pwsh).unwrap();
            writeln!(pwsh, "$ret=0").unwrap();
            writeln!(pwsh, "if (Test-Path {long_prog}) {{").unwrap();
            write_pwsh_invocation(
                &mut pwsh,
                &format!("{long_prog} {args} {quoted_target} $args"),
                "  ",
            );
            writeln!(pwsh, "  $ret=$LASTEXITCODE").unwrap();
            writeln!(pwsh, "}} else {{").unwrap();
            write_pwsh_invocation(
                &mut pwsh,
                &format!("{prog_quoted} {args} {quoted_target} $args"),
                "  ",
            );
            writeln!(pwsh, "  $ret=$LASTEXITCODE").unwrap();
            writeln!(pwsh, "}}").unwrap();
            write_pwsh_exit(&mut pwsh, restore_node_path, "$ret");
        }
        runtime_opt => {
            let args = runtime_opt.map_or("", |runtime| runtime.args.as_str());
            writeln!(pwsh).unwrap();
            write_pwsh_invocation(&mut pwsh, &format!("{quoted_target} {args} $args"), "");
            write_pwsh_exit(&mut pwsh, restore_node_path, "$LASTEXITCODE");
        }
    }

    pwsh
}

fn write_pwsh_invocation(pwsh: &mut String, command: &str, indent: &str) {
    use std::fmt::Write;
    writeln!(pwsh, "{indent}# Support pipeline input").unwrap();
    writeln!(pwsh, "{indent}if ($MyInvocation.ExpectingInput) {{").unwrap();
    writeln!(pwsh, "{indent}  $input | & {command}").unwrap();
    writeln!(pwsh, "{indent}}} else {{").unwrap();
    writeln!(pwsh, "{indent}  & {command}").unwrap();
    writeln!(pwsh, "{indent}}}").unwrap();
}

fn write_pwsh_exit(pwsh: &mut String, restore_node_path: Option<&str>, exit_code: &str) {
    use std::fmt::Write;
    if let Some(restore) = restore_node_path {
        writeln!(pwsh, "{restore}").unwrap();
    }
    writeln!(pwsh, "exit {exit_code}").unwrap();
}

fn quoted_pwsh_target(target_path: &Path, shim_path: &Path) -> String {
    let sh_target = relative_target(target_path, shim_path);
    if Path::new(&sh_target).is_absolute() {
        format!(r#""{sh_target}""#)
    } else {
        format!(r#""$basedir/{sh_target}""#)
    }
}

/// The shim header that also exports `NODE_PATH`, when there is one to
/// export.
fn pwsh_node_path_header(node_path: &[String]) -> Option<String> {
    let NodePathEnvVar { win32: win32_node_path, posix: posix_node_path } =
        normalize_node_path_env_var(node_path);
    (!win32_node_path.is_empty()).then(|| {
        format!(
            "#!/usr/bin/env pwsh\n$basedir=Split-Path $MyInvocation.MyCommand.Definition -Parent\n\n$exe=\"\"\n$pathsep=\":\"\n$env_node_path=$env:NODE_PATH\n$new_node_path=\"{win32_node_path}\"\nif ($PSVersionTable.PSVersion -lt \"6.0\" -or $IsWindows) {{\n  # Fix case when both the Windows and Linux builds of Node\n  # are installed in the same directory\n  $exe=\".exe\"\n  $pathsep=\";\"\n}} else {{\n  $new_node_path=\"{posix_node_path}\"\n}}\nif ([string]::IsNullOrEmpty($env_node_path)) {{\n  $env:NODE_PATH=$new_node_path\n}} else {{\n  $env:NODE_PATH=\"$new_node_path$pathsep$env_node_path\"\n}}",
        )
    })
}

/// `.ps1` template prelude. Sets up `$basedir` and `$exe`.
const PWSH_SHIM_HEADER: &str = r#"#!/usr/bin/env pwsh
$basedir=Split-Path $MyInvocation.MyCommand.Definition -Parent

$exe=""
if ($PSVersionTable.PSVersion -lt "6.0" -or $IsWindows) {
  # Fix case when both the Windows and Linux builds of Node
  # are installed in the same directory
  $exe=".exe"
}"#;
