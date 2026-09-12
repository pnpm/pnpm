use super::{
    BTreeMap, BTreeSet, DependencyGroup, PackageManifest, ProjectSnapshot, ResolvedDependencyMap,
    ResolvedDependencySpec, SpecDiff, StalenessReason, git_specifiers_are_equivalent,
};

/// Verify the on-disk `package.json` is still satisfied by the
/// lockfile's importer entry for the same project. Returns `Ok(())`
/// when the lockfile is up-to-date; returns `Err(StalenessReason)`
/// describing the first detected mismatch otherwise.
///
/// What is checked (in order, short-circuiting on the first failure):
///
/// 1. Flat-record specifier diff against `devDependencies ∪
///    dependencies ∪ optionalDependencies` (∪ the auto-installed
///    peers below). Catches added / removed / modified deps in one
///    bucket.
/// 2. `publishDirectory` / `linkDirectory` vs `publishConfig`.
/// 3. `dependenciesMeta` equality.
/// 4. Per-field name-set, per-dep specifier, and resolved-version
///    checks. Catches
///    same-name-same-specifier-but-listed-under-different-field
///    drift the flat-record diff doesn't see, plus broken lockfiles
///    whose resolved semver no longer satisfies the recorded range.
///
/// When `auto_install_peers` is set (pnpm's default), every peer
/// dependency missing from the regular dependency fields is folded
/// into `dependencies` for the comparison, matching how pnpm
/// materializes those peers into the importer's `dependencies` in the
/// lockfile. Without this, a peer-only dependency would be misread as
/// a lockfile entry the manifest removed (see `auto_installed_peer_deps`).
///
/// Still unsupported: `excludeLinksFromLockfile` (`link:` resolutions
/// aren't modeled yet). Non-semver resolutions such as file and
/// tarball dependencies are excluded from the resolved-version check.
pub fn satisfies_package_manifest(
    importer: &ProjectSnapshot,
    manifest: &PackageManifest,
    auto_install_peers: bool,
    is_ignored_optional: &dyn Fn(&str) -> bool,
) -> Result<(), StalenessReason> {
    let folded_peers = auto_installed_peer_deps(manifest, auto_install_peers);

    check_flat_specs(importer, manifest, &folded_peers, is_ignored_optional)?;
    check_publish_directory(importer, manifest)?;
    check_dependencies_meta(importer, manifest)?;
    check_dependency_fields(importer, manifest, &folded_peers, is_ignored_optional)
}

/// Phase 1: flat-record diff against the manifest's union of dependency
/// fields. Compares the importer's specifiers to the manifest's existing deps
/// (devs + prod + optional flattened together, plus the auto-installed
/// peers).
fn check_flat_specs(
    importer: &ProjectSnapshot,
    manifest: &PackageManifest,
    folded_peers: &BTreeMap<&str, &str>,
    is_ignored_optional: &dyn Fn(&str) -> bool,
) -> Result<(), StalenessReason> {
    let mut manifest_specs = flat_manifest_specs(manifest, is_ignored_optional);
    manifest_specs
        .extend(folded_peers.iter().map(|(name, spec)| ((*name).to_string(), (*spec).to_string())));
    let diff = diff_flat_records(&flat_importer_specs(importer), &manifest_specs);
    if diff.is_empty() {
        return Ok(());
    }
    Err(StalenessReason::SpecifiersDiffer(diff))
}

/// Phase 2: publish-directory parity. The directory is compared verbatim;
/// `linkDirectory` is compared by its effective value because omitted and
/// explicit `true` have the same behavior and only `false` is recorded.
fn check_publish_directory(
    importer: &ProjectSnapshot,
    manifest: &PackageManifest,
) -> Result<(), StalenessReason> {
    let manifest_publish_dir = manifest
        .value()
        .get("publishConfig")
        .and_then(|publish_config| publish_config.get("directory"))
        .and_then(|directory| directory.as_str())
        .map(str::to_owned);
    if importer.publish_directory != manifest_publish_dir {
        return Err(StalenessReason::PublishDirectoryMismatch {
            lockfile: importer.publish_directory.clone(),
            manifest: manifest_publish_dir,
        });
    }
    let lockfile_links_publish_directory =
        importer.publish_directory.is_some() && importer.link_directory != Some(false);
    let manifest_links_publish_directory = manifest_publish_dir.is_some()
        && manifest
            .value()
            .get("publishConfig")
            .and_then(|publish_config| publish_config.get("linkDirectory"))
            .and_then(serde_json::Value::as_bool)
            != Some(false);
    if lockfile_links_publish_directory != manifest_links_publish_directory {
        return Err(StalenessReason::LinkDirectoryMismatch {
            lockfile: lockfile_links_publish_directory,
            manifest: manifest_links_publish_directory,
        });
    }
    Ok(())
}

/// Phase 3: `dependenciesMeta` parity. JSON-equality of the two maps (or both
/// absent), so an absent map and an empty map are equivalent.
fn check_dependencies_meta(
    importer: &ProjectSnapshot,
    manifest: &PackageManifest,
) -> Result<(), StalenessReason> {
    let manifest_meta = manifest.value().get("dependenciesMeta");
    let importer_meta = importer.dependencies_meta.as_ref();
    if dependencies_meta_equal(importer_meta, manifest_meta) {
        return Ok(());
    }
    Err(StalenessReason::DependenciesMetaMismatch {
        lockfile: importer_meta.map_or_else(|| "{}".to_string(), std::string::ToString::to_string),
        manifest: manifest_meta.map_or_else(|| "{}".to_string(), std::string::ToString::to_string),
    })
}

/// Phase 4: per-field name-set + specifier match. The auto-installed peers
/// join `dependencies`, so they count toward the prod name-set used for both
/// the dev-field precedence filter and that field's own comparison.
fn check_dependency_fields(
    importer: &ProjectSnapshot,
    manifest: &PackageManifest,
    folded_peers: &BTreeMap<&str, &str>,
    is_ignored_optional: &dyn Fn(&str) -> bool,
) -> Result<(), StalenessReason> {
    let mut manifest_prod: BTreeMap<&str, &str> = manifest
        .dependencies([DependencyGroup::Prod])
        .filter(|(name, _)| !is_ignored_optional(name))
        .collect();
    manifest_prod.extend(folded_peers.iter().map(|(name, spec)| (*name, *spec)));
    let manifest_optional: BTreeMap<&str, &str> = manifest
        .dependencies([DependencyGroup::Optional])
        .filter(|(name, _)| !is_ignored_optional(name))
        .collect();

    for field in [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional] {
        let manifest_field = manifest_field_specs(
            manifest,
            field,
            &manifest_prod,
            &manifest_optional,
            folded_peers,
            is_ignored_optional,
        );
        let importer_field = importer.get_map_by_group(field);
        check_field_specs(&manifest_field, importer_field, field)?;
        check_field_extras(&manifest_field, importer_field, field)?;
    }

    Ok(())
}

/// One field's manifest entries after the precedence filter: a dependency
/// listed in several fields belongs to the most specific one.
fn manifest_field_specs<'a>(
    manifest: &'a PackageManifest,
    field: DependencyGroup,
    manifest_prod: &BTreeMap<&str, &str>,
    manifest_optional: &BTreeMap<&str, &str>,
    folded_peers: &BTreeMap<&'a str, &'a str>,
    is_ignored_optional: &dyn Fn(&str) -> bool,
) -> BTreeMap<&'a str, &'a str> {
    let mut specs: BTreeMap<&str, &str> = manifest
        .dependencies([field])
        .filter(|(name, _)| {
            !matches!(field, DependencyGroup::Prod | DependencyGroup::Optional)
                || !is_ignored_optional(name)
        })
        .filter(|(name, _)| match field {
            DependencyGroup::Dev => {
                !manifest_prod.contains_key(*name) && !manifest_optional.contains_key(*name)
            }
            DependencyGroup::Prod => !manifest_optional.contains_key(*name),
            DependencyGroup::Optional | DependencyGroup::Peer => true,
        })
        .collect();
    if matches!(field, DependencyGroup::Prod) {
        specs.extend(folded_peers.iter().map(|(name, spec)| (*name, *spec)));
    }
    specs
}

/// Every manifest entry must have a matching importer entry in the *same*
/// field with the same specifier, resolved to a version the specifier admits.
fn check_field_specs(
    manifest_field: &BTreeMap<&str, &str>,
    importer_field: Option<&ResolvedDependencyMap>,
    field: DependencyGroup,
) -> Result<(), StalenessReason> {
    let field_name = <&'static str>::from(field);
    for (name, manifest_spec) in manifest_field {
        let parsed = crate::PkgName::parse(*name).ok();
        let importer_dep =
            parsed.as_ref().and_then(|name| importer_field.and_then(|map| map.get(name)));
        let matched = importer_dep
            .is_some_and(|dep| dependency_specifiers_equal(&dep.specifier, manifest_spec));
        if !matched {
            return Err(StalenessReason::DepSpecifierMismatch {
                field: field_name,
                name: (*name).to_string(),
                lockfile: importer_dep
                    .map_or_else(|| "(absent)".to_string(), |dep| dep.specifier.clone()),
                manifest: (*manifest_spec).to_string(),
            });
        }
        check_resolution_satisfies(name, manifest_spec, importer_dep)?;
    }
    Ok(())
}

/// A specifier the importer's recorded resolution no longer satisfies means
/// the range moved under the lockfile.
fn check_resolution_satisfies(
    name: &str,
    manifest_spec: &str,
    importer_dep: Option<&ResolvedDependencySpec>,
) -> Result<(), StalenessReason> {
    let (Some(dep), Ok(range)) = (importer_dep, manifest_spec.parse::<node_semver::Range>()) else {
        return Ok(());
    };
    let Some(version) = dep.version.ver_peer().and_then(|version| version.version_semver()) else {
        return Ok(());
    };
    if range.satisfies(version) {
        return Ok(());
    }
    Err(StalenessReason::ResolutionDoesNotSatisfy {
        name: name.to_string(),
        version: version.to_string(),
        range: manifest_spec.to_string(),
    })
}

/// Every importer entry in this field must also exist in the manifest's same
/// field (post-precedence-filter). Catches the inverse of
/// [`check_field_specs`]: the lockfile lists a dependency here that the
/// manifest moved to a different field.
fn check_field_extras(
    manifest_field: &BTreeMap<&str, &str>,
    importer_field: Option<&ResolvedDependencyMap>,
    field: DependencyGroup,
) -> Result<(), StalenessReason> {
    let Some(importer_map) = importer_field else {
        return Ok(());
    };
    for (name, spec) in importer_map {
        if !manifest_field.contains_key(name.to_string().as_str()) {
            return Err(StalenessReason::DepSpecifierMismatch {
                field: <&'static str>::from(field),
                name: name.to_string(),
                lockfile: spec.specifier.clone(),
                manifest: "(absent)".to_string(),
            });
        }
    }
    Ok(())
}

/// Two `dependenciesMeta` maps are equal when both are absent / empty
/// or both render to the same JSON.
fn dependencies_meta_equal(
    importer: Option<&serde_json::Value>,
    manifest: Option<&serde_json::Value>,
) -> bool {
    fn is_empty_object(value: Option<&serde_json::Value>) -> bool {
        match value {
            None => true,
            Some(serde_json::Value::Object(map)) => map.is_empty(),
            Some(serde_json::Value::Null) => true,
            _ => false,
        }
    }
    match (importer, manifest) {
        (None, None) => true,
        (a, b) if is_empty_object(a) && is_empty_object(b) => true,
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// Peer dependencies that `auto-install-peers` materializes into the
/// importer's `dependencies`: every `peerDependencies` entry whose name
/// isn't already declared in `dependencies`, `devDependencies`, or
/// `optionalDependencies`. Empty when `auto_install_peers` is off, which
/// restores the plain manifest-vs-lockfile comparison.
///
/// Mirrors pnpm's `omit(Object.keys(existingDeps), pkg.peerDependencies)`
/// fold in `satisfiesPackageManifest`: peers already declared in a
/// regular field keep that field's specifier, so only the peer-only
/// entries are surfaced here.
pub(crate) fn auto_installed_peer_deps(
    manifest: &PackageManifest,
    auto_install_peers: bool,
) -> BTreeMap<&str, &str> {
    if !auto_install_peers {
        return BTreeMap::new();
    }
    let declared: BTreeSet<&str> = manifest
        .dependencies([DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional])
        .map(|(name, _)| name)
        .collect();
    manifest
        .dependencies([DependencyGroup::Peer])
        .filter(|(name, _)| !declared.contains(name))
        .collect()
}

/// Build the manifest's `devDependencies ∪ dependencies ∪
/// optionalDependencies` flat-record. Manifest fields are read in
/// dev → prod → optional order, but the order
/// is irrelevant for the diff since duplicates resolve to the same
/// specifier anyway — if two fields list the same name with different
/// specifiers the manifest is invalid and pacquet would have rejected
/// it earlier.
fn flat_manifest_specs(
    manifest: &PackageManifest,
    is_ignored_optional: &dyn Fn(&str) -> bool,
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for group in [DependencyGroup::Dev, DependencyGroup::Prod, DependencyGroup::Optional] {
        for (name, spec) in manifest.dependencies([group]) {
            if matches!(group, DependencyGroup::Prod | DependencyGroup::Optional)
                && is_ignored_optional(name)
            {
                continue;
            }
            out.insert(name.to_string(), spec.to_string());
        }
    }
    out
}

/// Build the importer's flat-record from its three dependency maps.
/// The inline-specifier shape of v9 lockfiles means each entry
/// already carries its `specifier` field; no top-level
/// `importer.specifiers` map is consulted (that's a v6/v7 shape that
/// pacquet's `ProjectSnapshot` still models for serde compatibility
/// but doesn't use here).
fn flat_importer_specs(importer: &ProjectSnapshot) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for group in [DependencyGroup::Dev, DependencyGroup::Prod, DependencyGroup::Optional] {
        if let Some(map) = importer.get_map_by_group(group) {
            for (name, spec) in map {
                out.insert(name.to_string(), spec.specifier.clone());
            }
        }
    }
    out
}

/// Bucket entries from two flat records into added/removed/modified.
/// `removed` is what's in `lockfile_specs` but missing from `manifest_specs`,
/// `added` is the inverse, `modified` are keys present in both but with
/// different values.
fn diff_flat_records(
    lockfile_specs: &BTreeMap<String, String>,
    manifest_specs: &BTreeMap<String, String>,
) -> SpecDiff {
    let lhs_keys: BTreeSet<&String> = lockfile_specs.keys().collect();
    let rhs_keys: BTreeSet<&String> = manifest_specs.keys().collect();
    let mut diff = SpecDiff::default();
    for k in lhs_keys.difference(&rhs_keys) {
        diff.removed.insert((**k).clone(), lockfile_specs[*k].clone());
    }
    for k in rhs_keys.difference(&lhs_keys) {
        diff.added.insert((**k).clone(), manifest_specs[*k].clone());
    }
    for k in lhs_keys.intersection(&rhs_keys) {
        let lhs_spec = &lockfile_specs[*k];
        let rhs_spec = &manifest_specs[*k];
        if !dependency_specifiers_equal(lhs_spec, rhs_spec) {
            diff.modified.insert((**k).clone(), (lhs_spec.clone(), rhs_spec.clone()));
        }
    }
    diff
}

pub(super) fn dependency_specifiers_equal(left: &str, right: &str) -> bool {
    left == right || git_specifiers_are_equivalent(left, right)
}
