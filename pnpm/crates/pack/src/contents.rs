use super::{
    FsFileLen, HashSet, Ordering, PackError, PackOptions, Path, PathBuf, Value,
    get_bins_from_package_manifest, is_manifest_entry,
};

/// Map each packed path to `package/<path>` → absolute source, in
/// packlist order.
pub(super) fn build_files_map(dir: &Path, files: &[String]) -> indexmap::IndexMap<String, PathBuf> {
    files.iter().map(|file| (format!("package/{file}"), dir.join(file))).collect()
}

/// Absolute source paths that should be marked executable in the
/// tarball: the publish manifest's resolved bins plus any
/// `publishConfig.executableFiles`.
pub(super) fn executable_sources(
    publish_manifest: &Value,
    manifest: &Value,
    dir: &Path,
) -> Vec<PathBuf> {
    let mut bins: Vec<PathBuf> =
        get_bins_from_package_manifest::<pnpm_cmd_shim::Host>(publish_manifest, dir)
            .into_iter()
            .map(|command| command.path)
            .collect();
    if let Some(executable_files) = manifest
        .get("publishConfig")
        .and_then(|config| config.get("executableFiles"))
        .and_then(Value::as_array)
    {
        for file in executable_files.iter().filter_map(Value::as_str) {
            bins.push(dir.join(file));
        }
    }
    bins
}

/// Append a workspace-root `LICENSE` to a sub-package tarball that lacks
/// one.
pub(super) fn inject_workspace_license(
    opts: &PackOptions,
    dir: &Path,
    files_map: &mut indexmap::IndexMap<String, PathBuf>,
) {
    let Some(workspace_dir) = &opts.workspace_dir else { return };
    if dir == workspace_dir || files_map.values().any(|file| contains_license(file)) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(workspace_dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_license_filename(&name) {
            continue;
        }
        // Only inject a regular file. A directory named `LICENSE` would
        // fail the later read/size pass with "Is a directory", and a
        // symlink could point outside the workspace and leak its target's
        // bytes into the published tarball. `DirEntry::file_type` does not
        // follow symlinks, so `is_file()` rejects both — matching the
        // symlink-skipping `read_readme_file` does in `exportable-manifest`.
        if entry.file_type().is_ok_and(|file_type| file_type.is_file()) {
            files_map.insert(format!("package/{name}"), workspace_dir.join(&name));
        }
    }
}

/// Total uncompressed size of every tar entry. Manifest entries use the
/// serialized publish manifest's length rather than the on-disk file's,
/// since `pack` rewrites them.
pub(super) fn unpacked_size<Sys: FsFileLen>(
    files_map: &indexmap::IndexMap<String, PathBuf>,
    manifest_json_len: u64,
) -> Result<u64, PackError> {
    let mut total = 0u64;
    for (name, source) in files_map {
        total += if is_manifest_entry(name) {
            manifest_json_len
        } else {
            Sys::file_len(source).map_err(|source_err| PackError::ReadFile {
                path: source.display().to_string(),
                source: source_err,
            })?
        };
    }
    Ok(total)
}

/// De-duplicated, locale-sorted list of the tarball's contents.
/// Manifest entries collapse to `package.json`; the `package/` prefix is
/// stripped from the rest.
/// [`packed_contents`] plus the injected entries' stripped names, re-sorted.
pub(super) fn packed_contents_with_injected(
    files_map: &indexmap::IndexMap<String, PathBuf>,
    injected: &[(String, Vec<u8>)],
) -> Vec<String> {
    let mut contents = packed_contents(files_map);
    for (name, _) in injected {
        let stripped = name.strip_prefix("package/").unwrap_or(name).to_string();
        if !contents.contains(&stripped) {
            contents.push(stripped);
        }
    }
    sort_paths_en_locale(&mut contents);
    contents
}

fn packed_contents(files_map: &indexmap::IndexMap<String, PathBuf>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut contents: Vec<String> = files_map
        .keys()
        .map(|name| {
            if is_manifest_entry(name) {
                "package.json".to_string()
            } else {
                name.strip_prefix("package/").unwrap_or(name).to_string()
            }
        })
        .filter(|item| seen.insert(item.clone()))
        .collect();
    sort_paths_en_locale(&mut contents);
    contents
}

/// Sort path strings the way pnpm's `localeCompare(b, 'en')` orders a
/// tarball's file listing: case-insensitively, with lowercase given
/// precedence over uppercase on case-only ties.
pub fn sort_paths_en_locale(paths: &mut Vec<String>) {
    // Decorate each path with its lowercase form once, rather than
    // recomputing `to_lowercase` for both sides on every comparison.
    let mut decorated: Vec<(String, String)> =
        std::mem::take(paths).into_iter().map(|item| (item.to_lowercase(), item)).collect();
    decorated.sort_by(|(left_lower, left), (right_lower, right)| {
        left_lower.cmp(right_lower).then_with(|| case_precedence_tiebreak(left, right))
    });
    *paths = decorated.into_iter().map(|(_, item)| item).collect();
}

/// Tie-breaker for [`sort_paths_en_locale`]'s `localeCompare(b, 'en')`
/// approximation: once two ASCII path strings compare equal
/// case-insensitively, give a lowercase character precedence over its
/// uppercase counterpart. Full ICU collation is not a workspace
/// dependency; this reproduces `en` ordering for plain file paths, where
/// the two agree.
fn case_precedence_tiebreak(left: &str, right: &str) -> Ordering {
    for (left_char, right_char) in left.chars().zip(right.chars()) {
        if left_char == right_char {
            continue;
        }
        return match (left_char.is_lowercase(), right_char.is_lowercase()) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => left_char.cmp(&right_char),
        };
    }
    left.len().cmp(&right.len())
}

/// Whether a packed path looks like a license file, matching upstream's
/// anchored `/(?:^|[\\/])LICEN[CS]E(?:\..+)?$/i` presence test.
fn contains_license(path: &Path) -> bool {
    if let Some(file_name) = path.file_name() {
        return is_license_filename(&file_name.to_string_lossy());
    }
    false
}

/// Whether a root filename matches the `LICEN{S,C}E{,.*}` glob pnpm
/// uses to find a workspace-root license to inject.
fn is_license_filename(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    matches!(lower.as_str(), "license" | "licence")
        || lower.starts_with("license.")
        || lower.starts_with("licence.")
}
