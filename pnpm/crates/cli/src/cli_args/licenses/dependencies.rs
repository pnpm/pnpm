use super::{
    BelongsTo, DepType, HashMap, Include, InstallabilityOptions, LicenseInfo, Lockfile, Ordering,
    PackageKey, ResolvedDependencyMap, WantedPlatformRef, detect_dep_types,
    platform_is_supported_with_inference,
};

pub(super) fn collect_dependencies(
    lockfile: &Lockfile,
    importer_ids: impl IntoIterator<Item = impl AsRef<str>>,
    include: Include,
    installability: &InstallabilityOptions<'_>,
) -> HashMap<PackageKey, BelongsTo> {
    let mut belongs_to: HashMap<PackageKey, BelongsTo> = HashMap::new();
    let mut stack: Vec<(PackageKey, BelongsTo)> = Vec::new();
    for id in importer_ids {
        let Some(importer) =
            lockfile.importers.get(id.as_ref()).or_else(|| lockfile.root_project())
        else {
            continue;
        };
        queue_importer_deps(importer, include, &mut stack);
    }

    walk_installed_closure(lockfile, include, installability, stack, &mut belongs_to);

    // A package reachable only through `devDependencies` is dev whatever
    // edge kind first reached it here.
    let dep_types = detect_dep_types(lockfile);
    for (key, belongs_to) in &mut belongs_to {
        *belongs_to = if dep_types.get(key) == Some(&DepType::DevOnly) {
            BelongsTo::Dev
        } else {
            BelongsTo::Prod
        };
    }

    belongs_to
}

/// Walk the seeded stack, recording every package the install would
/// materialize with the broadest edge kind that reaches it.
fn walk_installed_closure(
    lockfile: &Lockfile,
    include: Include,
    installability: &InstallabilityOptions<'_>,
    mut stack: Vec<(PackageKey, BelongsTo)>,
    belongs_to: &mut HashMap<PackageKey, BelongsTo>,
) {
    let empty_snapshots = HashMap::new();
    let snapshots = lockfile.snapshots.as_ref().unwrap_or(&empty_snapshots);
    while let Some((key, kind)) = stack.pop() {
        if let Some(existing) = belongs_to.get(&key)
            && *existing <= kind
        {
            continue;
        }
        let snapshot = snapshots.get(&key);
        if snapshot_is_unsupported_optional(lockfile, &key, snapshot, installability) {
            continue;
        }
        belongs_to.insert(key.clone(), kind);
        if let Some(snapshot) = snapshot {
            queue_snapshot_children(snapshot, kind, include, &mut stack);
        }
    }
}

/// Seed the walk with one importer's direct dependencies, in the groups
/// the command includes.
fn queue_importer_deps(
    importer: &pnpm_lockfile::ProjectSnapshot,
    include: Include,
    stack: &mut Vec<(PackageKey, BelongsTo)>,
) {
    let mut queue_deps = |deps: Option<&ResolvedDependencyMap>, kind: BelongsTo| {
        for (alias, spec) in deps.into_iter().flatten() {
            if let Some(key) = spec.version.resolved_key(alias) {
                stack.push((key, kind));
            }
        }
    };
    if include.dependencies {
        queue_deps(importer.dependencies.as_ref(), BelongsTo::Prod);
    }
    if include.dev_dependencies {
        queue_deps(importer.dev_dependencies.as_ref(), BelongsTo::Dev);
    }
    if include.optional_dependencies {
        queue_deps(importer.optional_dependencies.as_ref(), BelongsTo::Optional);
    }
}

/// One snapshot's children, inheriting the edge kind that reached it.
fn queue_snapshot_children(
    snapshot: &pnpm_lockfile::SnapshotEntry,
    kind: BelongsTo,
    include: Include,
    stack: &mut Vec<(PackageKey, BelongsTo)>,
) {
    let optional = include.optional_dependencies.then_some(snapshot.optional_dependencies.as_ref());
    for deps in [Some(snapshot.dependencies.as_ref()), optional].into_iter().flatten().flatten() {
        for (name, dep_ref) in deps {
            if let Some(child_key) = dep_ref.resolve(name) {
                stack.push((child_key, kind));
            }
        }
    }
}

/// Whether the package is an optional dependency this host cannot
/// install, and so is not part of the installed license set.
fn snapshot_is_unsupported_optional(
    lockfile: &Lockfile,
    key: &PackageKey,
    snapshot: Option<&pnpm_lockfile::SnapshotEntry>,
    installability: &InstallabilityOptions<'_>,
) -> bool {
    if !snapshot.is_some_and(|snapshot| snapshot.optional) {
        return false;
    }
    let package = lockfile.packages.as_ref().and_then(|packages| packages.get(&key.without_peer()));
    package.is_some_and(|package| {
        !platform_is_supported_with_inference(
            &key.name.bare,
            WantedPlatformRef {
                os: package.os.as_deref(),
                cpu: package.cpu.as_deref(),
                libc: package.libc.as_deref(),
            },
            installability,
        )
    })
}

fn version_is_newer(candidate: &str, selected: &str) -> bool {
    compare_versions(candidate, selected).is_gt()
}

pub(super) fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    match (node_semver::Version::parse(left), node_semver::Version::parse(right)) {
        (Ok(left), Ok(right)) => left.cmp(&right),
        _ => left.cmp(right),
    }
}

pub(super) fn compare_package_names(left: &str, right: &str) -> Ordering {
    left.bytes()
        .map(package_name_collation_weight)
        .cmp(right.bytes().map(package_name_collation_weight))
        .then_with(|| {
            left.bytes()
                .zip(right.bytes())
                .find_map(|(left, right)| {
                    if left == right || !left.eq_ignore_ascii_case(&right) {
                        None
                    } else if left.is_ascii_lowercase() {
                        Some(Ordering::Less)
                    } else {
                        Some(Ordering::Greater)
                    }
                })
                .unwrap_or_else(|| left.cmp(right))
        })
}

fn package_name_collation_weight(byte: u8) -> u8 {
    match byte {
        b'_' => 0,
        b'-' => 1,
        b'.' => 2,
        b'@' => 3,
        b'/' => 4,
        b'~' => 5,
        byte => byte.to_ascii_lowercase().saturating_add(6),
    }
}

pub(super) fn select_newer_version(
    info: &mut LicenseInfo,
    candidate_version: &str,
    candidate_belongs_to: BelongsTo,
) -> bool {
    if !version_is_newer(candidate_version, &info.selected_version) {
        return false;
    }
    info.belongs_to = candidate_belongs_to;
    info.selected_version = candidate_version.to_string();
    true
}
