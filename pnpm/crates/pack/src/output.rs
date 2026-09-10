use super::{
    PackError, PackFile, PackOptions, PackResult, PackResultJson, Path, PathBuf, Value,
    lexical_normalize,
};

/// Project a [`PackResult`] into its JSON shape.
#[must_use]
pub fn to_pack_result_json(result: &PackResult) -> PackResultJson {
    let manifest = &result.published_manifest;
    PackResultJson {
        name: manifest.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
        version: manifest.get("version").and_then(Value::as_str).unwrap_or_default().to_string(),
        filename: result.tarball_path.clone(),
        files: result.contents.iter().map(|path| PackFile { path: path.clone() }).collect(),
    }
}

/// Render packed results the way `pnpm pack` prints them: pretty JSON
/// under `--json`, otherwise a per-package "Tarball Contents / Details"
/// block.
#[must_use]
pub fn format_pack_output(results: &[PackResultJson], json: bool, unicode: bool) -> String {
    if json {
        return if results.len() > 1 {
            serde_json::to_string_pretty(&results)
        } else {
            serde_json::to_string_pretty(&results[0])
        }
        .expect("pack result serializes to JSON");
    }

    let prefix = if unicode { "📦 " } else { "package:" };
    results
        .iter()
        .map(|result| {
            // `name` / `version` / `filename` and the file paths are
            // manifest- and filesystem-derived, so strip control
            // characters before they reach the terminal — a file named
            // with raw ANSI escapes would otherwise spoof the output.
            let files = result
                .files
                .iter()
                .map(|file| sanitize_for_terminal(&file.path))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "{prefix} {name}@{version}\nTarball Contents\n{files}\nTarball Details\n{filename}",
                name = sanitize_for_terminal(&result.name),
                version = sanitize_for_terminal(&result.version),
                filename = sanitize_for_terminal(&result.filename),
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Strip control characters (keeping `\n` / `\t`) from text headed for the
/// terminal, so a manifest- or filesystem-derived value can't emit raw
/// escape sequences. JSON output is left untouched — it is data, not a
/// terminal rendering.
fn sanitize_for_terminal(text: &str) -> std::borrow::Cow<'_, str> {
    if text
        .chars()
        .any(|character| character.is_control() && character != '\n' && character != '\t')
    {
        std::borrow::Cow::Owned(
            text.chars()
                .filter(|character| {
                    !character.is_control() || *character == '\n' || *character == '\t'
                })
                .collect(),
        )
    } else {
        std::borrow::Cow::Borrowed(text)
    }
}

/// `name.replace('@', '').replace('/', '-')`, first-occurrence only, to
/// match the JS `String.prototype.replace(string, ...)` semantics that
/// build a tarball's default filename.
pub(super) fn normalize_tarball_name(name: &str) -> String {
    name.replacen('@', "", 1).replacen('/', "-", 1)
}

/// Resolve `(tarball_name, pack_destination)` from the `--out` template
/// or the default `<name>-<version>.tgz`. `--out` and
/// `--pack-destination` are mutually exclusive.
pub(super) fn resolve_output(
    opts: &PackOptions,
    normalized_name: &str,
    version: &str,
) -> Result<(String, Option<String>), PackError> {
    resolve_output_values(
        opts.out.as_deref(),
        opts.pack_destination.as_deref(),
        normalized_name,
        version,
    )
}

fn resolve_output_values(
    out: Option<&str>,
    pack_destination: Option<&str>,
    normalized_name: &str,
    version: &str,
) -> Result<(String, Option<String>), PackError> {
    let Some(out) = out else {
        return Ok((
            format!("{normalized_name}-{version}.tgz"),
            pack_destination.map(str::to_owned),
        ));
    };
    if pack_destination.is_some() {
        return Err(PackError::OutAndPackDestination);
    }
    let prepared = out.replace("%s", normalized_name).replace("%v", version);
    let prepared_path = Path::new(&prepared);
    // `--out .`, `--out ..`, or `--out ""` resolve to no filename; the
    // join would then target a directory and the write would fail with a
    // confusing OS error, so reject the option up front.
    let Some(tarball_name) =
        prepared_path.file_name().map(|name| name.to_string_lossy().into_owned())
    else {
        return Err(PackError::InvalidOut { out: out.to_owned() });
    };
    let parent =
        prepared_path.parent().map(|dir| dir.to_string_lossy().into_owned()).unwrap_or_default();
    let pack_destination =
        if parent.is_empty() { pack_destination.map(str::to_owned) } else { Some(parent) };
    Ok((tarball_name, pack_destination))
}

/// Resolve the path a pack will write from the manifest identity and output
/// options. Recursive callers use this before dispatch to keep projects that
/// share a destination from packing concurrently.
pub fn pack_output_path(
    project_dir: &Path,
    out: Option<&str>,
    pack_destination: Option<&str>,
    published_name: &str,
    published_version: &str,
) -> Result<PathBuf, PackError> {
    let normalized_name = normalize_tarball_name(published_name);
    let version = strip_build_metadata(published_version);
    let (tarball_name, destination) =
        resolve_output_values(out, pack_destination, &normalized_name, version)?;
    Ok(lexical_normalize(&resolve_dest_dir(project_dir, destination.as_deref()).join(tarball_name)))
}

/// Resolve the directory the tarball is written into.
pub(super) fn resolve_dest_dir(dir: &Path, pack_destination: Option<&str>) -> PathBuf {
    match pack_destination {
        Some(destination) if Path::new(destination).is_absolute() => PathBuf::from(destination),
        Some(destination) => dir.join(destination),
        None => dir.to_path_buf(),
    }
}

/// The reported tarball path: relative to the project root when the
/// tarball landed there, otherwise the absolute destination path.
pub(super) fn packed_tarball_path(
    project_dir: &Path,
    publish_dir: &Path,
    dest_dir: &Path,
    tarball_name: &str,
) -> String {
    if project_dir != dest_dir {
        return dest_dir.join(tarball_name).display().to_string();
    }
    pathdiff::diff_paths(publish_dir.join(tarball_name), project_dir)
        .unwrap_or_else(|| PathBuf::from(tarball_name))
        .display()
        .to_string()
}

/// `version` without its `+<build>` metadata segment.
pub(super) fn strip_build_metadata(version: &str) -> &str {
    version.split_once('+').map_or(version, |(base, _)| base)
}

/// Resolve a path's realpath, falling back to the input when it doesn't
/// exist yet. Mirrors upstream's `realpathMissing` for the lifecycle
/// `INIT_CWD`-adjacent modules dir.
pub(super) fn realpath_missing(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}
