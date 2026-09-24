use super::{
    BTreeMap, HashMap, HashSet, OsStr, PacklistError, Path, PathBuf, Value, VecDeque,
    collect_own_files, fs, safe_read_package_json_from_dir,
};

/// Cap on `bundleDependencies` closure depth. Real packages bundle
/// at most a handful of levels (most published packages bundle zero;
/// the rare ones bundle one or two). Reusing a package that is already
/// visible makes the walk terminate; this cap is belt-and-braces against a
/// pathological tree that keeps resolving fresh canonical paths
/// (e.g. a deep chain of `dependencies` that never repeats).
pub(super) const MAX_BUNDLE_DEPTH: u32 = 32;

/// One unit of `bundleDependencies`-closure work: resolve `name` for the
/// already-placed package `parent`, then splice the resolved package's files
/// into the output.
struct BundleTask {
    name: String,
    parent: usize,
    depth: u32,
}

/// A package the closure has placed in the packed tree.
struct PlacedPackage {
    /// Package names from the packed root down to this package, so
    /// `["a", "b"]` is packed at `node_modules/a/node_modules/b`.
    packed: Vec<String>,
    /// Real directory, where the Node resolution walk for its dependencies starts.
    real_dir: PathBuf,
}

/// Where the closure may look for packages and what it has placed so far.
struct BundleWalk<'a> {
    pkg_dir: &'a Path,
    real_pkg_dir: &'a Path,
    boundary: &'a Path,
    /// Names the packed root depends on without bundling them at the top.
    /// A transitive bundle is not hoisted over them.
    root_dependency_names: HashSet<String>,
    placed: Vec<PlacedPackage>,
    /// Real directory of the package at each packed location.
    slots: HashMap<Vec<String>, PathBuf>,
}

/// Build the `bundleDependencies` closure for the package at `pkg_dir` and
/// splice each bundled package's files into `out`, keyed by its location in
/// the packed tree and mapped to the file it is read from.
///
/// Mirrors [`npm-bundled`](https://github.com/npm/npm-bundled): seed
/// from the root manifest's bundle list, then transitively pull in
/// every reachable dependency. Once a package is bundled, its own
/// `dependencies` and `optionalDependencies` are bundled too.
/// `devDependencies` are never followed.
///
/// Each dependency resolves the way Node resolves it at runtime: from the
/// parent's real directory, walking up through ancestor `node_modules`
/// directories to `boundary`. The isolated linker keeps a package's
/// dependencies next to its real directory rather than under the link, so the
/// resolved directory can sit anywhere under `boundary`, and the packed location
/// is chosen separately by [`BundleWalk::packed_location`].
pub(super) fn collect_bundled_files(
    pkg_dir: &Path,
    root_manifest: &Value,
    boundary: &Path,
    out: &mut BTreeMap<String, PathBuf>,
) -> Result<(), PacklistError> {
    let Some(real_pkg_dir) = fs::canonicalize(pkg_dir).ok() else { return Ok(()) };
    let Some(boundary) = fs::canonicalize(boundary).ok() else { return Ok(()) };
    let mut walk = BundleWalk {
        pkg_dir,
        real_pkg_dir: &real_pkg_dir,
        boundary: &boundary,
        root_dependency_names: dependency_names(root_manifest).into_iter().collect(),
        placed: vec![PlacedPackage { packed: Vec::new(), real_dir: real_pkg_dir.clone() }],
        slots: HashMap::new(),
    };
    let mut queue: VecDeque<BundleTask> = root_bundle_dep_names(root_manifest)
        .into_iter()
        .map(|name| BundleTask { name, parent: 0, depth: 0 })
        .collect();

    while let Some(task) = queue.pop_front() {
        let Some(admitted) = admitted_bundle(&task, &walk) else {
            continue;
        };
        let parent = &walk.placed[task.parent];
        let Some(packed) = walk.packed_location(&task.name, &parent.packed, &admitted) else {
            continue;
        };
        let dep_manifest = walk.pack(packed, admitted, out)?;
        let placed = walk.placed.len() - 1;
        for name in dependency_names(&dep_manifest) {
            queue.push_back(BundleTask { name, parent: placed, depth: task.depth + 1 });
        }
    }
    Ok(())
}

impl BundleWalk<'_> {
    /// Splice the files of `dependency` into `out` at `packed`, and return its
    /// manifest.
    fn pack(
        &mut self,
        packed: Vec<String>,
        dependency: AdmittedBundle,
        out: &mut BTreeMap<String, PathBuf>,
    ) -> Result<Value, PacklistError> {
        let AdmittedBundle { dir, real_dir } = dependency;
        let dir = dir
            .strip_prefix(self.real_pkg_dir)
            .map_or_else(|_| dir.clone(), |rel| self.pkg_dir.join(rel));
        let prefix = packed_dir(&packed);
        let manifest = safe_read_package_json_from_dir(&dir)
            .ok()
            .flatten()
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
        for rel in collect_own_files(&dir, &manifest, None)? {
            out.insert(format!("{prefix}/{rel}"), dir.join(rel));
        }
        self.slots.insert(packed.clone(), real_dir.clone());
        self.placed.push(PlacedPackage { packed, real_dir });
        Ok(manifest)
    }

    /// The packed location for `dependency`, required from the package packed
    /// at `parent`, so that Node resolves it from `parent` inside the tarball.
    /// Prefers the on-disk location when that already works, then the
    /// top-level `node_modules`, then `parent`'s own `node_modules`.
    ///
    /// `None` when a package at the same real directory is already visible
    /// from `parent`, which also ends dependency cycles, or when no location
    /// can make it visible.
    fn packed_location(
        &self,
        name: &str,
        parent: &[String],
        dependency: &AdmittedBundle,
    ) -> Option<Vec<String>> {
        let mut visible_free_slots = Vec::new();
        for depth in (0..=parent.len()).rev() {
            let mut slot = parent[..depth].to_vec();
            slot.push(name.to_string());
            match self.slots.get(&slot) {
                Some(real_dir) if *real_dir == dependency.real_dir => return None,
                Some(_) => break,
                None => visible_free_slots.push(slot),
            }
        }
        let on_disk = packed_names(self.real_pkg_dir, &dependency.real_dir);
        if let Some(slot) = on_disk.filter(|slot| visible_free_slots.contains(slot)) {
            return Some(slot);
        }
        let top_level = vec![name.to_string()];
        let may_hoist = parent.is_empty() || !self.root_dependency_names.contains(name);
        if may_hoist && visible_free_slots.contains(&top_level) {
            return Some(top_level);
        }
        let own = visible_free_slots.into_iter().next();
        if own.is_none() {
            tracing::warn!(
                target: "pacquet::fs_packlist",
                bundle_name = %name,
                "bundled dependency is shadowed by a different package of the same name; skipping",
            );
        }
        own
    }
}

/// `node_modules/<a>/node_modules/<b>` for the packed location `["a", "b"]`.
fn packed_dir(packed: &[String]) -> String {
    let mut dir = String::new();
    for name in packed {
        if !dir.is_empty() {
            dir.push('/');
        }
        dir.push_str("node_modules/");
        dir.push_str(name);
    }
    dir
}

/// The packed location that mirrors `dir`'s place under `pkg_dir`, when `dir`
/// is `pkg_dir/node_modules/<a>/node_modules/<b>/...`.
fn packed_names(pkg_dir: &Path, dir: &Path) -> Option<Vec<String>> {
    let rel = dir.strip_prefix(pkg_dir).ok()?;
    let mut segments = rel
        .components()
        .map(|component| component.as_os_str().to_str());
    let mut names = Vec::new();
    while let Some(segment) = segments.next() {
        if segment? != "node_modules" {
            return None;
        }
        let name = segments.next()??;
        if name.starts_with('.') {
            return None;
        }
        if name.starts_with('@') {
            names.push(format!("{name}/{}", segments.next()??));
        } else {
            names.push(name.to_string());
        }
    }
    (!names.is_empty()).then_some(names)
}

/// The directory one bundled dependency resolves to, once it has passed every
/// check that keeps the closure inside the resolution boundary. `None` for an
/// entry the walk refuses or cannot resolve, which is warned about here.
///
/// A malicious manifest could carry `bundleDependencies: ["../../etc"]` or an
/// absolute path, so the name must be a single safe segment before it reaches
/// a join. That only screens the name: a `node_modules/<name>` symlink
/// pointing at a sibling or an absolute host path passes it yet resolves
/// outside the boundary, and walking that would splice host files into the
/// published set. The fetcher imports untrusted git-hosted packages, so this
/// matters.
fn admitted_bundle(task: &BundleTask, walk: &BundleWalk<'_>) -> Option<AdmittedBundle> {
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
    let from_dir = &walk.placed[task.parent].real_dir;
    let Some(dep_dir) = resolve_bundled_dependency(&task.name, from_dir, walk.boundary) else {
        tracing::debug!(
            target: "pacquet::fs_packlist",
            bundle_name = %task.name,
            from_dir = %from_dir.display(),
            "bundleDependencies entry not resolvable under node_modules/; skipping",
        );
        return None;
    };
    // A dependency whose real path cannot be read cannot be proven to stay
    // inside the boundary, so it is refused.
    let real_dir = fs::canonicalize(&dep_dir)
        .ok()
        .filter(|real| real.starts_with(walk.boundary));
    let Some(real_dir) = real_dir else {
        tracing::warn!(
            target: "pacquet::fs_packlist",
            bundle_name = %task.name,
            dep_dir = %dep_dir.display(),
            "bundled dependency resolves outside the package tree; refusing",
        );
        return None;
    };
    Some(AdmittedBundle { dir: dep_dir, real_dir })
}

/// A bundled dependency the walk accepted.
struct AdmittedBundle {
    /// The `node_modules/<name>` entry the resolution walk found.
    dir: PathBuf,
    /// Its real path, which placement compares on.
    real_dir: PathBuf,
}

/// Resolve a bundled dependency `name` to its directory using the
/// node module-resolution walk-up: check `from_dir/node_modules/name`,
/// then climb to each ancestor's `node_modules/`, stopping at `boundary`.
/// Returns the first directory that contains a `package.json`.
fn resolve_bundled_dependency(name: &str, from_dir: &Path, boundary: &Path) -> Option<PathBuf> {
    let mut current = from_dir;
    loop {
        if current.file_name() != Some(OsStr::new("node_modules")) {
            let candidate = current.join("node_modules").join(name);
            if candidate.join("package.json").is_file() {
                return Some(candidate);
            }
        }
        if current == boundary {
            return None;
        }
        current = current.parent()?;
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
    let raw = manifest
        .get("bundleDependencies")
        .or_else(|| manifest.get("bundledDependencies"));
    let Some(raw) = raw else { return Vec::new() };
    match raw {
        Value::Array(arr) => arr
            .iter()
            .filter_map(Value::as_str)
            .map(String::from)
            .collect(),
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
fn dependency_names(manifest: &Value) -> Vec<String> {
    let mut names = Vec::new();
    for field in ["dependencies", "optionalDependencies"] {
        if let Some(map) = manifest.get(field).and_then(Value::as_object) {
            names.extend(map.keys().cloned());
        }
    }
    names
}
