use crate::capabilities::FsWalkFiles;
use pacquet_fs::is_subdir;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// One bin entry resolved from a package's `package.json`.
///
/// `name` is the command name as it should appear under `node_modules/.bin/`.
/// `path` is the absolute path to the script the shim invokes.
///
/// Mirrors `Command` in pnpm v11's `@pnpm/bins.resolver`:
/// <https://github.com/pnpm/pnpm/blob/4750fd370c/bins/resolver/src/index.ts>.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub name: String,
    pub path: PathBuf,
}

/// Bin names that legitimately ship inside a different package than their own
/// name. Mirrors `BIN_OWNER_OVERRIDES` in
/// <https://github.com/pnpm/pnpm/blob/4750fd370c/bins/resolver/src/index.ts>.
///
/// Used by [`pkg_owns_bin`] for conflict resolution between two packages
/// declaring the same bin name.
const BIN_OWNER_OVERRIDES: &[(&str, &[&str])] = &[
    ("npx", &["npm"]),
    ("pn", &["pnpm", "@pnpm/exe"]),
    ("pnpm", &["@pnpm/exe"]),
    ("pnpx", &["pnpm", "@pnpm/exe"]),
    ("pnx", &["pnpm", "@pnpm/exe"]),
];

/// Whether `pkg_name` is a legitimate owner of the given `bin_name`. The
/// default rule is "the package named `X` owns the `X` bin"; overrides cover
/// cases like `npx` shipping inside `npm`. Mirrors `pkgOwnsBin`.
#[must_use]
pub fn pkg_owns_bin(bin_name: &str, pkg_name: &str) -> bool {
    if bin_name == pkg_name {
        return true;
    }
    BIN_OWNER_OVERRIDES
        .iter()
        .find(|(name, _)| *name == bin_name)
        .is_some_and(|(_, owners)| owners.contains(&pkg_name))
}

/// Read every bin declared by `manifest` and return them as [`Command`]s
/// rooted at `pkg_path`.
///
/// Handles the three cases pnpm supports, in order:
///
/// 1. `bin` as a string. The bin name is the package's own `name` (with any
///    `@scope/` prefix stripped). Empty / missing `name` skips the entry, in
///    parity with pnpm's `INVALID_PACKAGE_NAME` guard.
/// 2. `bin` as an object. Each `(commandName, relativePath)` becomes a
///    command, with `@scope/` stripped from the key.
/// 3. Fallback: `directories.bin`. Every regular file under the directory
///    becomes a command, with the file basename as the bin name. The
///    directory itself must resolve under `pkg_path`; a `directories.bin`
///    that escapes via `..` returns an empty list.
///
/// Validation, exactly mirroring pnpm:
///
/// - Bin name must be URL-safe (`name == encodeURIComponent(name)`) or be the
///   single-character `$`. This is the path-traversal guard.
/// - Bin path must resolve under `pkg_path`. Prevents a malicious manifest
///   from writing shims that exec a sibling package.
pub fn get_bins_from_package_manifest<Sys: FsWalkFiles>(
    manifest: &Value,
    pkg_path: &Path,
) -> Vec<Command> {
    let pkg_name = manifest.get("name").and_then(Value::as_str);
    if let Some(bin) = manifest.get("bin") {
        return commands_from_bin(bin, pkg_name, pkg_path);
    }
    if let Some(bin_dir_rel) =
        manifest.get("directories").and_then(|d| d.get("bin")).and_then(Value::as_str)
    {
        return commands_from_directories_bin::<Sys>(bin_dir_rel, pkg_path);
    }
    Vec::new()
}

/// Walk every regular file under `<pkg_path>/<bin_dir_rel>` and emit one
/// [`Command`] per file. Mirrors pnpm's `findFiles` + the `directories.bin`
/// branch in `getBinsFromPackageManifest`:
/// <https://github.com/pnpm/pnpm/blob/4750fd370c/bins/resolver/src/index.ts>.
///
/// Symlinks are not followed; pnpm uses `tinyglobby` with
/// `followSymbolicLinks: false`. Missing directory degrades to an empty
/// list (pnpm's `ENOENT` short-circuit).
fn commands_from_directories_bin<Sys: FsWalkFiles>(
    bin_dir_rel: &str,
    pkg_path: &Path,
) -> Vec<Command> {
    let bin_dir = pkg_path.join(bin_dir_rel);
    if !is_subdir(pkg_path, &bin_dir) {
        return Vec::new();
    }
    // Treat a top-level walk error as "no bins". This matches pnpm's
    // tinyglobby ENOENT short-circuit. The trait's production impl
    // already drops per-entry errors inside its iterator, so an `Err`
    // here only fires when the walker can't even open `bin_dir`.
    let Ok(paths) = Sys::walk_files(&bin_dir) else {
        return Vec::new();
    };
    let mut commands = Vec::new();
    for path in paths {
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        // Same URL-safe-name guard as the keyed-bin path.
        if !is_safe_bin_name(name) {
            continue;
        }
        commands.push(Command { name: name.to_string(), path });
    }
    commands
}

fn commands_from_bin(bin: &Value, pkg_name: Option<&str>, pkg_path: &Path) -> Vec<Command> {
    let mut entries: Vec<(String, String)> = Vec::new();
    match bin {
        Value::String(rel_path) => {
            let Some(name) = pkg_name else {
                return Vec::new();
            };
            entries.push((name.to_string(), rel_path.clone()));
        }
        Value::Object(map) => {
            for (key, value) in map {
                let Some(rel_path) = value.as_str() else {
                    continue;
                };
                entries.push((key.clone(), rel_path.to_string()));
            }
        }
        _ => return Vec::new(),
    }

    let mut commands = Vec::with_capacity(entries.len());
    for (command_name, bin_relative_path) in entries {
        // Strip any `@scope/` prefix. Mirrors `commandsFromBin`'s
        // `commandName[0] === '@'` branch.
        let bin_name = if command_name.starts_with('@') {
            match command_name.find('/') {
                Some(slash) => command_name[slash + 1..].to_string(),
                None => command_name,
            }
        } else {
            command_name
        };

        if !is_safe_bin_name(&bin_name) {
            continue;
        }

        let bin_path = pkg_path.join(&bin_relative_path);
        if !is_subdir(pkg_path, &bin_path) {
            continue;
        }

        commands.push(Command { name: bin_name, path: bin_path });
    }
    commands
}

/// Whether `name` matches the URL-safe character set allowed by JavaScript's
/// `encodeURIComponent`, or is the single-character escape hatch `$` pnpm
/// permits for awkward but legitimate bin names. Together these are the only
/// names pnpm allows the linker to write to disk.
///
/// `encodeURIComponent` leaves the following bytes unescaped:
/// `A-Z a-z 0-9 - _ . ! ~ * ' ( )`.
///
/// `.` and `..` survive `encodeURIComponent` unchanged but resolve to the bin
/// directory itself or its parent when joined to a target dir, so they are
/// rejected explicitly.
fn is_safe_bin_name(name: &str) -> bool {
    if name == "$" {
        return true;
    }
    if name.is_empty() || name == "." || name == ".." {
        return false;
    }
    name.bytes().all(|byte| {
        byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')')
    })
}

#[cfg(test)]
mod tests;
