use super::{Compression, GzEncoder, Path, PathBuf, Range, Value, Version, WalkDir, Write, fs, io};

pub(super) fn build_tarball(
    root: &Path,
    package_dir: &Path,
    manifest: &Value,
    manifest_text: &str,
) -> Vec<u8> {
    let name = manifest.get("name").and_then(Value::as_str).unwrap_or_default();
    let gzip = GzEncoder::new(Vec::new(), Compression::default());
    let mut tar = tar::Builder::new(gzip);
    for entry in fixture_files(package_dir) {
        let relative = entry.path().strip_prefix(package_dir).expect("fixture entry under package");
        let path_in_archive = Path::new("package").join(relative);
        let content = if relative == Path::new("package.json") {
            manifest_text.as_bytes().to_vec()
        } else {
            fs::read(entry.path()).expect("read fixture file")
        };
        let mode = file_mode(root, entry.path(), &content).expect("read fixture file mode");
        append_file(&mut tar, &path_in_archive, &content, mode);
    }
    // Files whose names differ only by case cannot coexist in a case-insensitive
    // working tree (the default on macOS and Windows), so they are composed into
    // the archive here instead of being committed as colliding fixture files.
    for (relative, content) in in_memory_files(name) {
        let path_in_archive = Path::new("package").join(relative);
        append_file(&mut tar, &path_in_archive, content.as_bytes(), 0o644);
    }
    // pnpm's `publish` copies the workspace-root LICENSE into every package that
    // doesn't ship its own; registry-mock published these fixtures that way, so
    // reproduce the injected LICENSE here.
    if should_inject_root_license(name) && !package_dir.join("LICENSE").exists() {
        append_file(&mut tar, Path::new("package/LICENSE"), INJECTED_LICENSE.as_bytes(), 0o644);
    }
    // `bundleDependencies` packages publish their resolved dependency tree inside
    // the tarball's `node_modules`. registry-mock produces this with a
    // `prepublishOnly` install; reproduce it here so `node_modules` (gitignored)
    // never has to be committed.
    for (relative, content, mode) in bundled_node_modules(root, manifest) {
        let path_in_archive = Path::new("package").join(relative);
        append_file(&mut tar, &path_in_archive, &content, mode);
    }
    let gzip = tar.into_inner().expect("finish tar archive");
    gzip.finish().expect("finish gzip archive")
}

pub(super) const INJECTED_LICENSE: &str = include_str!("../../../../LICENSE");

// The bundle-dependency fixtures publish via a `prepublishOnly` install that
// turns each into a self-contained workspace with no root LICENSE to copy, and
// a couple of special fixtures were likewise published without one. Everything
// else receives the injected root LICENSE, matching the registry-mock tarballs.
pub(super) fn should_inject_root_license(name: &str) -> bool {
    !matches!(
        name,
        "@pnpm.e2e/pkg-with-bundle-dependencies"
            | "@pnpm.e2e/pkg-with-bundle-dependencies-true"
            | "@pnpm.e2e/pkg-with-bundle-dependencies-false"
            | "@pnpm.e2e/pkg-with-bundled-dependencies"
            | "@pnpm.e2e/pkg-with-accidentally-published-catalog-protocol",
    )
}

pub(super) fn in_memory_files(name: &str) -> &'static [(&'static str, &'static str)] {
    match name {
        "@pnpm.e2e/with-same-file-in-different-cases" => {
            &[("Foo.js", "// Foo.js\n"), ("foo.js", "// foo.js\n")]
        }
        _ => &[],
    }
}

pub(super) fn bundled_node_modules(root: &Path, manifest: &Value) -> Vec<(PathBuf, Vec<u8>, u32)> {
    let mut files = Vec::new();
    for dep in bundled_dependency_names(manifest) {
        let spec = manifest
            .get("dependencies")
            .and_then(|deps| deps.get(&dep))
            .and_then(Value::as_str)
            .unwrap_or("*");
        let Some(version) = resolve_fixture_version(root, &dep, spec) else { continue };
        let dep_dir = root.join(&dep).join(&version);
        for entry in fixture_files(&dep_dir) {
            let relative = entry
                .path()
                .strip_prefix(&dep_dir)
                .expect("bundled dependency entry under dep dir");
            let archive = Path::new("node_modules").join(&dep).join(relative);
            let content = fs::read(entry.path()).expect("read bundled dependency file");
            let mode =
                file_mode(root, entry.path(), &content).expect("bundled dependency file mode");
            files.push((archive, content, mode));
        }
    }
    files
}

pub(super) fn bundled_dependency_names(manifest: &Value) -> Vec<String> {
    let bundled =
        manifest.get("bundleDependencies").or_else(|| manifest.get("bundledDependencies"));
    match bundled {
        Some(Value::Bool(true)) => manifest
            .get("dependencies")
            .and_then(Value::as_object)
            .map(|deps| deps.keys().cloned().collect())
            .unwrap_or_default(),
        Some(Value::Array(names)) => {
            names.iter().filter_map(|name| name.as_str().map(String::from)).collect()
        }
        _ => Vec::new(),
    }
}

pub(super) fn resolve_fixture_version(root: &Path, dep: &str, spec: &str) -> Option<String> {
    let range = Range::parse(spec).ok()?;
    let mut best: Option<(Version, String)> = None;
    for entry in fs::read_dir(root.join(dep)).ok()? {
        let raw = entry.ok()?.file_name().to_string_lossy().into_owned();
        let Ok(version) = Version::parse(&raw) else { continue };
        if range.satisfies(&version) && best.as_ref().is_none_or(|(best, _)| version > *best) {
            best = Some((version, raw));
        }
    }
    best.map(|(_, raw)| raw)
}

pub(super) fn fixture_files(root: &Path) -> Vec<walkdir::DirEntry> {
    let mut entries: Vec<_> = WalkDir::new(root)
        .into_iter()
        .map(|entry| entry.expect("walk registry package fixtures"))
        .filter(|entry| entry.file_type().is_file())
        .collect();
    entries.sort_by(|left, right| left.path().cmp(right.path()));
    entries
}

pub(super) fn append_file<Writer: Write>(
    tar: &mut tar::Builder<Writer>,
    path_in_archive: &Path,
    content: &[u8],
    mode: u32,
) {
    let mut header = tar::Header::new_gnu();
    header.set_size(content.len() as u64);
    header.set_mode(mode);
    header.set_cksum();
    tar.append_data(&mut header, path_in_archive, content).expect("append fixture file");
}

pub(super) fn file_mode(root: &Path, source: &Path, content: &[u8]) -> io::Result<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(source)?.permissions().mode() & 0o777;
        if mode & 0o111 != 0 {
            return Ok(mode);
        }
    }
    let relative = source.strip_prefix(root).expect("fixture source under root");
    if content.starts_with(b"#!")
        || relative.components().any(|component| component.as_os_str() == "bin")
    {
        return Ok(0o755);
    }
    Ok(0o644)
}
