use std::{collections::BTreeMap, path::Path};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Maximum number of commands precalculated in `dist.pnpr.bins`. Packages
/// declaring more commands fall back to on-disk bin discovery at link time.
pub const MAX_PRECALCULATED_BINS: usize = 64;

/// Maximum serialized JSON byte size of a single version's `dist.pnpr.bins`.
pub const MAX_PRECALCULATED_BINS_BYTES: usize = 8 * 1024;

/// Detected runtime for a target script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptRuntime {
    /// The interpreter to invoke. `None` means "exec the file directly".
    pub prog: Option<String>,
    /// Extra arguments declared after the interpreter in the shebang. Empty
    /// when the runtime came from the extension fallback.
    pub args: String,
}

/// One precalculated bin entry for a package, as serialized in `dist.pnpr.bins`
/// and cached during tarball extraction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageBin {
    /// Normalized relative path from the package root using `/` separators
    /// (for example `"bin/cli.js"`).
    pub path: String,
    /// Interpreter to invoke (`None` when the target should be executed
    /// directly).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prog: Option<String>,
    /// Extra arguments declared after the interpreter (including leading
    /// whitespace from a shebang, or `"/C"` for `.cmd`/`.bat` extension
    /// fallback).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub args: String,
}

impl PackageBin {
    /// Whether this bin entry satisfies the invariants of a normalized
    /// package-relative path and a shell-safe single-line runtime.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        let valid_path = !self.path.is_empty()
            && normalize_package_relative_path(&self.path).as_deref() == Some(self.path.as_str());
        let valid_prog = match &self.prog {
            Some(prog) => {
                !prog.is_empty() && !prog.starts_with('-') && prog.bytes().all(is_valid_prog_byte)
            }
            None => self.args.is_empty(),
        };
        let valid_args = self.args.bytes().all(is_valid_args_byte);
        valid_path && valid_prog && valid_args
    }

    /// Convert this bin entry's runtime fields into the [`ScriptRuntime`]
    /// expected by the shim generator.
    #[must_use]
    pub fn script_runtime(&self) -> Option<ScriptRuntime> {
        if self.prog.is_none() && self.args.is_empty() {
            None
        } else {
            Some(ScriptRuntime { prog: self.prog.clone(), args: self.args.clone() })
        }
    }

    #[must_use]
    pub fn from_path_and_runtime(path: String, runtime: Option<ScriptRuntime>) -> Self {
        match runtime {
            Some(ScriptRuntime { prog, args }) => Self { path, prog, args },
            None => Self { path, prog: None, args: String::new() },
        }
    }
}

fn is_valid_prog_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-' | b'+')
}

fn is_valid_args_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(byte, b' ' | b'\t' | b'/' | b'.' | b'_' | b'-' | b'+' | b'=' | b':' | b',')
}

/// Map of file extensions to their default runtime when the script lacks a
/// shebang.
#[must_use]
pub fn extension_program(extension: &str) -> Option<&'static str> {
    match extension {
        "js" | "cjs" | "mjs" => Some("node"),
        "cmd" | "bat" => Some("cmd"),
        "ps1" => Some("pwsh"),
        "sh" => Some("sh"),
        _ => None,
    }
}

/// Combine an already-parsed shebang runtime with the extension fallback for
/// `path`.
#[must_use]
pub fn infer_script_runtime_from_shebang(
    path: &Path,
    shebang: Option<ScriptRuntime>,
) -> Option<ScriptRuntime> {
    if let Some(rt) = shebang {
        return Some(rt);
    }
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("");
    let prog = extension_program(extension)?;
    let args = if prog == "cmd" { "/C" } else { "" };
    Some(ScriptRuntime { prog: Some(prog.to_string()), args: args.to_string() })
}

/// Infer the runtime for `path` from up to the first 512 bytes of its content,
/// falling back to its file extension when no shebang is present.
#[must_use]
pub fn infer_script_runtime_from_head(path: &Path, head_bytes: &[u8]) -> Option<ScriptRuntime> {
    infer_script_runtime_from_shebang(path, parse_shebang_from_bytes(head_bytes))
}

/// Parse the runtime out of the first line of a script's content. Pure
/// function over bytes so the caller can plug in any I/O strategy.
///
/// Does **not** trim leading whitespace before looking for `#!`. The
/// kernel treats `#!` as a shebang only when it sits at byte 0 of the
/// file; trimming would accept inputs like
/// `" \n#!/usr/bin/env node"` as a valid shebang and could select the
/// wrong runtime for files that just happen to mention `#!` after some
/// whitespace.
#[must_use]
pub fn parse_shebang_from_bytes(bytes: &[u8]) -> Option<ScriptRuntime> {
    if !bytes.starts_with(b"#!") {
        return None;
    }
    let line_bytes = match bytes
        .iter()
        .position(|&byte| byte == b'\n')
    {
        Some(idx) => &bytes[..idx],
        None => bytes,
    };
    let first_line = String::from_utf8_lossy(line_bytes);
    parse_shebang(first_line.trim_end_matches('\r'))
}

/// Parses the shebang against the grammar
/// `^#!\s*(?:/usr/bin/env(?:\s+-S\s*)?)?\s*([^ \t]+)(.*)$`.
///
/// `args` is captured **including the leading whitespace** that
/// separates it from `prog` — everything from after `prog`'s end of
/// match to end of line. Preserving the leading whitespace is what
/// produces byte-identical shim text (e.g. the double space between
/// `$basedir/sh` and `-e` in the rendered exec line).
#[must_use]
pub fn parse_shebang(line: &str) -> Option<ScriptRuntime> {
    let rest = line.strip_prefix("#!")?.trim_start();
    let (rest, _) = strip_env_prefix(rest);
    let rest = rest.trim_start();

    let (prog, args) = match rest.find([' ', '\t']) {
        Some(idx) => rest.split_at(idx),
        None => (rest, ""),
    };

    if prog.is_empty() {
        return None;
    }

    Some(ScriptRuntime { prog: Some(prog.to_string()), args: args.to_string() })
}

/// Strip a leading `/usr/bin/env`, optionally followed by `-S`, from the
/// shebang body. Returns the remainder and whether `env` was present.
fn strip_env_prefix(input: &str) -> (&str, bool) {
    let Some(rest) = input.strip_prefix("/usr/bin/env") else {
        return (input, false);
    };
    if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
        return (input, false);
    }
    let trimmed = rest.trim_start();
    if let Some(after_dash_s) = trimmed.strip_prefix("-S")
        && (after_dash_s.is_empty() || after_dash_s.starts_with([' ', '\t']))
    {
        return (after_dash_s, true);
    }
    (trimmed, true)
}

/// The `<command>` / `<relative path>` pairs a `bin` field declares. A string
/// `bin` names one command after the package itself, so a package with no name
/// declares none.
#[must_use]
pub fn declared_bin_entries(bin: &Value, pkg_name: Option<&str>) -> Vec<(String, String)> {
    match bin {
        Value::String(rel_path) => pkg_name
            .map(|name| vec![(name.to_string(), rel_path.clone())])
            .unwrap_or_default(),
        Value::Object(map) => map
            .iter()
            .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_string())))
            .collect(),
        _ => Vec::new(),
    }
}

/// The bin name a command is linked under: a scoped command drops its scope,
/// since the scope is not part of the file name in `.bin`.
#[must_use]
pub fn unscoped_bin_name(command_name: String) -> String {
    if !command_name.starts_with('@') {
        return command_name;
    }
    match command_name.find('/') {
        Some(slash) => command_name[slash + 1..].to_string(),
        None => command_name,
    }
}

/// Whether `name` matches the URL-safe character set allowed by JavaScript's
/// `encodeURIComponent`, or is the single-character escape hatch `$` pnpm
/// permits for awkward but legitimate bin names. Together these are the only
/// names pnpm allows the linker to write to disk.
#[must_use]
pub fn is_safe_bin_name(name: &str) -> bool {
    if name == "$" {
        return true;
    }
    if name.is_empty() || name == "." || name == ".." {
        return false;
    }
    name.bytes()
        .all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')')
        })
}

/// Normalize a package-relative path using `/` separators, rejecting paths
/// that start with `/`, contain `\` or `:`, or climb above the package root
/// via `..`.
#[must_use]
pub fn normalize_package_relative_path(raw: &str) -> Option<String> {
    if raw.starts_with('/') || raw.contains(['\\', ':']) {
        return None;
    }
    let mut stack = Vec::new();
    for segment in raw.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                stack.pop()?;
            }
            normal => stack.push(normal),
        }
    }
    Some(stack.join("/"))
}

/// Whether `manifest` declares any bins via `"bin"` or `"directories.bin"`.
#[must_use]
pub fn manifest_declares_bins(manifest: &Value) -> bool {
    if let Some(bin) = manifest
        .get("bin")
        .filter(|bin| bin.as_str() != Some(""))
    {
        return bin.is_string() || bin.is_object();
    }
    manifest
        .get("directories")
        .and_then(|dirs| dirs.get("bin"))
        .and_then(Value::as_str)
        .is_some()
}

/// Precalculate the package's resolved bins and shebang/extension runtimes
/// from its `package.json` and the archive's regular files (`cleaned_path` ->
/// parsed shebang).
#[must_use]
pub fn resolve_package_bins(
    manifest: &Value,
    archive_files: &BTreeMap<String, Option<ScriptRuntime>>,
) -> Option<BTreeMap<String, PackageBin>> {
    resolve_package_bins_with(
        manifest,
        |path| archive_files.contains_key(path),
        archive_files.keys().map(String::as_str),
        |path| {
            archive_files
                .get(path)
                .cloned()
                .flatten()
        },
    )
}

/// Precalculate the package's resolved bins and shebang/extension runtimes
/// using caller-supplied file-presence, directory-walk, and shebang-lookup
/// callbacks.
#[must_use]
pub fn resolve_package_bins_with<'files>(
    manifest: &Value,
    has_file: impl Fn(&str) -> bool,
    file_paths: impl IntoIterator<Item = &'files str>,
    shebang_for: impl Fn(&str) -> Option<ScriptRuntime>,
) -> Option<BTreeMap<String, PackageBin>> {
    if !manifest_declares_bins(manifest) {
        return Some(BTreeMap::new());
    }
    let bin_paths = resolve_package_bin_paths(manifest, has_file, file_paths)?;
    let mut bins = BTreeMap::new();
    for (bin_name, normalized) in bin_paths {
        let shebang = shebang_for(&normalized);
        let runtime = infer_script_runtime_from_shebang(Path::new(&normalized), shebang);
        let pkg_bin = PackageBin::from_path_and_runtime(normalized, runtime);
        if !pkg_bin.is_valid() {
            return None;
        }
        bins.insert(bin_name, pkg_bin);
    }
    is_within_precalculated_bins_budget(&bins).then_some(bins)
}

/// Precalculate the package's resolved bins and shebang/extension runtimes
/// from its `package.json` and `{relative_path -> cas_path}` map by reading
/// up to the first 512 bytes of each resolved bin target from disk.
#[must_use]
pub fn resolve_package_bins_from_cas_paths<Hasher: std::hash::BuildHasher>(
    manifest: &Value,
    cas_paths: &std::collections::HashMap<String, std::path::PathBuf, Hasher>,
) -> Option<BTreeMap<String, PackageBin>> {
    if !manifest_declares_bins(manifest) {
        return Some(BTreeMap::new());
    }
    let bin_paths = resolve_package_bin_paths(
        manifest,
        |path| cas_paths.contains_key(path),
        cas_paths.keys().map(String::as_str),
    )?;
    let mut bins = BTreeMap::new();
    let mut head = [0u8; 512];
    for (bin_name, normalized) in bin_paths {
        let cas_path = cas_paths.get(&normalized)?;
        let filled = read_file_head(cas_path, &mut head)?;
        let runtime = infer_script_runtime_from_head(Path::new(&normalized), &head[..filled]);
        let pkg_bin = PackageBin::from_path_and_runtime(normalized, runtime);
        if !pkg_bin.is_valid() {
            return None;
        }
        bins.insert(bin_name, pkg_bin);
    }
    is_within_precalculated_bins_budget(&bins).then_some(bins)
}

/// Whether `bins` satisfies the per-version command-count and serialized
/// byte-size limits for `dist.pnpr.bins`.
#[must_use]
pub fn is_within_precalculated_bins_budget(bins: &BTreeMap<String, PackageBin>) -> bool {
    bins.len() <= MAX_PRECALCULATED_BINS
        && serde_json::to_vec(bins)
            .is_ok_and(|encoded| encoded.len() <= MAX_PRECALCULATED_BINS_BYTES)
}

/// Serialize `bins` to a JSON [`Value`] along with its compact JSON byte
/// length when `bins` is non-empty and within [`MAX_PRECALCULATED_BINS`] and
/// [`MAX_PRECALCULATED_BINS_BYTES`].
#[must_use]
pub fn serialize_precalculated_bins(bins: &BTreeMap<String, PackageBin>) -> Option<(Value, usize)> {
    if bins.is_empty() || bins.len() > MAX_PRECALCULATED_BINS {
        return None;
    }
    let encoded = serde_json::to_vec(bins).ok()?;
    if encoded.len() > MAX_PRECALCULATED_BINS_BYTES {
        return None;
    }
    let value = serde_json::to_value(bins).ok()?;
    Some((value, encoded.len()))
}

fn read_file_head(path: &Path, buf: &mut [u8]) -> Option<usize> {
    use std::io::Read as _;

    let mut file = std::fs::File::open(path).ok()?;
    let mut filled = 0;
    while filled < buf.len() {
        match file.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
    Some(filled)
}

/// Verify that precalculated `bins` match the command names, normalized
/// file paths, and shebang/extension runtimes declared by `manifest` and
/// the CAS files in `cas_paths`.
#[must_use]
pub fn verify_package_bins<Hasher: std::hash::BuildHasher>(
    bins: &BTreeMap<String, PackageBin>,
    manifest: &Value,
    cas_paths: &std::collections::HashMap<String, std::path::PathBuf, Hasher>,
) -> bool {
    resolve_package_bins_from_cas_paths(manifest, cas_paths).as_ref() == Some(bins)
}

fn resolve_package_bin_paths<'files>(
    manifest: &Value,
    has_file: impl Fn(&str) -> bool,
    file_paths: impl IntoIterator<Item = &'files str>,
) -> Option<BTreeMap<String, String>> {
    let pkg_name = manifest.get("name").and_then(Value::as_str);
    if let Some(bin) = manifest
        .get("bin")
        .filter(|bin| bin.as_str() != Some(""))
    {
        return resolve_paths_from_bin_field(bin, pkg_name, has_file);
    }
    if let Some(bin_dir_rel) = manifest
        .get("directories")
        .and_then(|dirs| dirs.get("bin"))
        .and_then(Value::as_str)
    {
        return resolve_paths_from_directories_bin(bin_dir_rel, file_paths);
    }
    Some(BTreeMap::new())
}

fn resolve_paths_from_bin_field(
    bin: &Value,
    pkg_name: Option<&str>,
    has_file: impl Fn(&str) -> bool,
) -> Option<BTreeMap<String, String>> {
    let mut paths = BTreeMap::new();
    for (command_name, bin_relative_path) in declared_bin_entries(bin, pkg_name) {
        let bin_name = unscoped_bin_name(command_name);
        if !is_safe_bin_name(&bin_name) {
            continue;
        }
        let normalized = normalize_package_relative_path(&bin_relative_path)?;
        if normalized.is_empty() || !has_file(&normalized) || paths.contains_key(&bin_name) {
            return None;
        }
        paths.insert(bin_name, normalized);
        if paths.len() > MAX_PRECALCULATED_BINS {
            return None;
        }
    }
    Some(paths)
}

fn resolve_paths_from_directories_bin<'files>(
    bin_dir_rel: &str,
    file_paths: impl IntoIterator<Item = &'files str>,
) -> Option<BTreeMap<String, String>> {
    let bin_dir = normalize_package_relative_path(bin_dir_rel)?;
    let mut paths = BTreeMap::new();
    for file_path in file_paths {
        let Some(file_name) = bin_dir_entry_name(file_path, &bin_dir) else {
            continue;
        };
        if paths.insert(file_name.to_string(), file_path.to_string()).is_some()
            || paths.len() > MAX_PRECALCULATED_BINS
        {
            return None;
        }
    }
    Some(paths)
}

fn bin_dir_entry_name<'path>(file_path: &'path str, bin_dir: &str) -> Option<&'path str> {
    let in_bin_dir = bin_dir.is_empty()
        || file_path
            .strip_prefix(bin_dir)
            .is_some_and(|rest| rest.starts_with('/'));
    if !in_bin_dir {
        return None;
    }
    let file_name = file_path
        .rsplit('/')
        .next()
        .unwrap_or(file_path);
    is_safe_bin_name(file_name).then_some(file_name)
}

#[cfg(test)]
mod tests;
