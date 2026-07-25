//! Probe the runtimes installed on the system, mirroring
//! `@pnpm/engine.runtime.system-version`.

use std::process::Command;

/// The version of `runtime` as installed on the system, without a leading
/// `v`, or `None` when the runtime is not on `PATH` or prints something
/// unparsable.
pub(crate) fn system_runtime_version(runtime: &str) -> Option<String> {
    match runtime {
        "node" => pacquet_graph_hasher::detect_node_version(),
        "deno" => run_version_command("deno").as_deref().and_then(parse_deno_version),
        "bun" => run_version_command("bun").as_deref().and_then(parse_bun_version),
        _ => None,
    }
}

/// Resolve `program` on `PATH` and run `<program> --version`. The lookup is
/// explicit because a bare [`Command::new`] on Windows also searches the
/// current directory, which would let a runtime pin in an untrusted
/// repository run a `deno.exe` / `bun.exe` checked in beside it.
fn run_version_command(program: &str) -> Option<String> {
    let program = which::which(program).ok()?;
    let output = Command::new(program).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(std::str::from_utf8(&output.stdout).ok()?.to_string())
}

/// `deno --version` prints several lines, the first of them
/// `deno <version> (release, <target>)`.
fn parse_deno_version(stdout: &str) -> Option<String> {
    stdout.lines().find_map(|line| {
        let rest = line.strip_prefix("deno")?;
        if !rest.starts_with(char::is_whitespace) {
            return None;
        }
        let version = rest.split_whitespace().next()?;
        accept_version(version)
    })
}

/// `bun --version` prints the bare version and nothing else.
fn parse_bun_version(stdout: &str) -> Option<String> {
    accept_version(stdout.trim())
}

/// `text` when it is a whole semver version. The probed binary is only as
/// trustworthy as `PATH`, and its output is echoed back in the
/// runtime-mismatch message, so anything that is not a version — including
/// a version followed by terminal escapes — is rejected outright rather
/// than reported as the installed version.
fn accept_version(text: &str) -> Option<String> {
    node_semver::Version::parse(text).ok().map(|_| text.to_string())
}

#[cfg(test)]
mod tests;
