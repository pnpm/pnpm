use super::{
    BTreeSet, HashSet, PacklistError, Path, PathBuf, Value, VecDeque, collect_own_files, fs,
    relative_forward_slash, safe_read_package_json_from_dir,
};

/// Cap on `bundleDependencies` closure depth. Real packages bundle
/// at most a handful of levels (most published packages bundle zero;
/// the rare ones bundle one or two). The visited-set already makes
/// the walk terminate; this cap is belt-and-braces against a
/// pathological tree that keeps resolving fresh canonical paths
/// (e.g. a deep chain of `dependencies` that never repeats).
pub(super) const MAX_BUNDLE_DEPTH: u32 = 32;

/// One unit of `bundleDependencies`-closure work: resolve `name`
/// starting the node module-resolution walk-up at `from_dir`, then
/// splice the resolved package's files into the output.
struct BundleTask {
    name: String,
    from_dir: PathBuf,
    depth: u32,
}

/// Build the `bundleDependencies` closure for `root` and splice each
/// bundled package's files into `out` under the package's real path
/// relative to `root` (e.g. `node_modules/<name>/...`).
///
/// Mirrors [`npm-bundled`](https://github.com/npm/npm-bundled): seed
/// from the root manifest's bundle list, then transitively pull in
/// every reachable dependency. Once a package is bundled, its own
/// `dependencies` and `optionalDependencies` are bundled too — that
/// is how the closure reaches a hoisted transitive dep sitting at the
/// root `node_modules/`. `devDependencies` are never followed.
///
/// The `visited` set is keyed on the canonicalised resolved directory,
/// so a diamond (two bundled deps sharing a transitive dep) processes
/// the shared package once and a `dependencies` cycle terminates
/// instead of looping forever.
pub(super) fn collect_bundled_files(
    root: &Path,
    root_manifest: &Value,
    out: &mut BTreeSet<String>,
) -> Result<(), PacklistError> {
    // Canonical form of the package root, used to reject any bundled
    // dependency whose real path escapes the tree (see the symlink check
    // in the loop below). `None` if `root` itself can't be canonicalised,
    // in which case the escape check falls back to a lexical comparison.
    let canonical_root = fs::canonicalize(root).ok();
    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut queue: VecDeque<BundleTask> = root_bundle_dep_names(root_manifest)
        .into_iter()
        .map(|name| BundleTask { name, from_dir: root.to_path_buf(), depth: 0 })
        .collect();

    while let Some(task) = queue.pop_front() {
        let Some(admitted) = admitted_bundle(&task, root, canonical_root.as_deref()) else {
            continue;
        };
        let AdmittedBundle { dir: dep_dir, dedup_key } = admitted;
        if !visited.insert(dedup_key) {
            continue;
        }
        let prefix = relative_forward_slash(root, &dep_dir);
        let dep_manifest = safe_read_package_json_from_dir(&dep_dir)
            .ok()
            .flatten()
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
        for rel in collect_own_files(&dep_dir, &dep_manifest, None)? {
            out.insert(format!("{prefix}/{rel}"));
        }
        for name in nested_bundle_dep_names(&dep_manifest) {
            queue.push_back(BundleTask { name, from_dir: dep_dir.clone(), depth: task.depth + 1 });
        }
    }
    Ok(())
}

/// The directory one bundled dependency resolves to, once it has passed every
/// check that keeps the closure inside the package tree. `None` for an entry
/// the walk refuses or cannot resolve, which is warned about here.
///
/// A malicious manifest could carry `bundleDependencies: ["../../etc"]` or an
/// absolute path, so the name must be a single safe segment before it reaches
/// a join. That only screens the name: a `node_modules/<name>` symlink
/// pointing at a sibling or an absolute host path passes it yet resolves
/// outside the tree, and walking that would splice host files into the
/// published set. The fetcher imports untrusted git-hosted packages, so this
/// matters.
fn admitted_bundle(
    task: &BundleTask,
    root: &Path,
    canonical_root: Option<&Path>,
) -> Option<AdmittedBundle> {
    if task.depth > MAX_BUNDLE_DEPTH {
        tracing::warn!(
            target: "pacquet::fs_packlist",
            bundle_name = %task.name,
            depth = task.depth,
            "bundleDependencies closure exceeded MAX_BUNDLE_DEPTH; refusing to descend further",
        );
        return None;
    }
    if !is_safe_bundle_name(&task.name) {
        tracing::warn!(
            target: "pacquet::fs_packlist",
            bundle_name = %task.name,
            "rejecting bundleDependencies entry that is not a single path segment",
        );
        return None;
    }
    let Some(dep_dir) = resolve_bundled_dependency(&task.name, &task.from_dir, root) else {
        tracing::debug!(
            target: "pacquet::fs_packlist",
            bundle_name = %task.name,
            from_dir = %task.from_dir.display(),
            "bundleDependencies entry not resolvable under node_modules/; skipping",
        );
        return None;
    };
    // `fs::canonicalize` resolves symlinks, giving both the escape check its
    // real target and the walk its dedup key: a symlink loop shows up as an
    // already-visited path. `None` on failure (permission denied, say), and
    // both then degrade — the check to a lexical comparison, the dedup to the
    // raw path.
    let canonical_dep = fs::canonicalize(&dep_dir).ok();
    if escapes_package_tree(&dep_dir, root, canonical_root, canonical_dep.as_deref()) {
        tracing::warn!(
            target: "pacquet::fs_packlist",
            bundle_name = %task.name,
            dep_dir = %dep_dir.display(),
            "bundled dependency resolves outside the package tree; refusing",
        );
        return None;
    }
    let dedup_key = canonical_dep.unwrap_or_else(|| dep_dir.clone());
    Some(AdmittedBundle { dir: dep_dir, dedup_key })
}

/// A bundled dependency the walk accepted.
struct AdmittedBundle {
    dir: PathBuf,
    /// The real path, which the walk dedups on.
    dedup_key: PathBuf,
}

/// Whether a bundled dependency's real path lies outside the package root.
fn escapes_package_tree(
    dep_dir: &Path,
    root: &Path,
    canonical_root: Option<&Path>,
    canonical_dep: Option<&Path>,
) -> bool {
    let Some(canonical_root) = canonical_root else {
        // Root itself won't canonicalise (pathological): fall back to a
        // best-effort lexical check.
        return dep_dir.strip_prefix(root).is_err();
    };
    // Root resolved but the dependency's real path didn't: we cannot prove it
    // stays inside the tree, so fail closed. A genuine dependency always
    // canonicalises here — `resolve_bundled_dependency` already stat'd its
    // `package.json` through the same path.
    let Some(canonical_dep) = canonical_dep else {
        return true;
    };
    canonical_dep.strip_prefix(canonical_root).is_err()
}

/// Resolve a bundled dependency `name` to its directory using the
/// node module-resolution walk-up: check `from_dir/node_modules/name`,
/// then climb to each ancestor's `node_modules/`, stopping at `root`.
/// Returns the first directory that contains a `package.json`, or
/// `None` if the name resolves nowhere within the package tree.
///
/// Climbing past `root` is refused so a hoisted dep always resolves to
/// the package being packed rather than to a sibling on the host.
fn resolve_bundled_dependency(name: &str, from_dir: &Path, root: &Path) -> Option<PathBuf> {
    let mut current = from_dir.to_path_buf();
    loop {
        let candidate = current.join("node_modules").join(name);
        if candidate.join("package.json").is_file() {
            return Some(candidate);
        }
        if current == root {
            return None;
        }
        let parent = current.parent()?;
        if parent == current {
            return None;
        }
        current = parent.to_path_buf();
    }
}

/// True when `name` is a safe `bundleDependencies` entry — the join
/// `pkg_dir/node_modules/<name>` stays inside `pkg_dir/node_modules`.
///
/// Rejects parent-dir components, root, and drive prefixes. Accepts
/// scoped names like `@scope/foo`: those legitimately carry a slash
/// and resolve to `pkg_dir/node_modules/@scope/foo`, which is still
/// inside the package tree. Same component-based discipline
/// `cas_io::join_checked` uses for tarball entries.
pub(super) fn is_safe_bundle_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let path = std::path::Path::new(name);
    if path.is_absolute() {
        return false;
    }
    for component in path.components() {
        match component {
            std::path::Component::Normal(_) => {}
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => {
                return false;
            }
            // `.` components are stripped silently — `./foo` resolves
            // the same as `foo` on every platform.
            std::path::Component::CurDir => {}
        }
    }
    true
}

/// Seed names for the bundle closure: the root manifest's
/// `bundleDependencies` (or the legacy `bundledDependencies`). Both
/// spellings appear in real published packages; npm-packlist accepts
/// either.
fn root_bundle_dep_names(manifest: &Value) -> Vec<String> {
    let raw = manifest.get("bundleDependencies").or_else(|| manifest.get("bundledDependencies"));
    let Some(raw) = raw else { return Vec::new() };
    match raw {
        Value::Array(arr) => arr.iter().filter_map(Value::as_str).map(String::from).collect(),
        Value::Bool(true) => {
            // `bundleDependencies: true` means "bundle every entry in
            // `dependencies`". Rare but supported by npm. Materialize
            // the keys from the dependencies map.
            manifest
                .get("dependencies")
                .and_then(Value::as_object)
                .map(|map| map.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

/// Names a bundled package pulls into the closure: every key in its
/// own `dependencies` and `optionalDependencies`. A bundled package
/// ships its whole runtime closure, so these are followed regardless
/// of whether the nested package declares its own `bundleDependencies`
/// (already-bundled packages don't re-gate their deps). `peer`- and
/// `dev`-dependencies are deliberately excluded — they are not part of
/// the published closure. Mirrors `npm-bundled`'s `getDeps`.
fn nested_bundle_dep_names(manifest: &Value) -> Vec<String> {
    let mut names = Vec::new();
    for field in ["dependencies", "optionalDependencies"] {
        if let Some(map) = manifest.get(field).and_then(Value::as_object) {
            names.extend(map.keys().cloned());
        }
    }
    names
}
