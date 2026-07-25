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

fn run_version_command(program: &str) -> Option<String> {
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
        starts_with_version(version).then(|| version.to_string())
    })
}

/// `bun --version` prints the bare version and nothing else.
fn parse_bun_version(stdout: &str) -> Option<String> {
    let version = stdout.trim();
    starts_with_version(version).then(|| version.to_string())
}

/// Whether `text` starts with `<digits>.<digits>.<digit>` — the shape both
/// upstream probes require before accepting the reported version.
fn starts_with_version(text: &str) -> bool {
    let mut parts = text.splitn(3, '.');
    let is_number = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    parts.next().is_some_and(is_number)
        && parts.next().is_some_and(is_number)
        && parts.next().is_some_and(|patch| patch.starts_with(|char: char| char.is_ascii_digit()))
}

#[cfg(test)]
mod tests;
