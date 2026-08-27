//! Moving an update's declared ranges onto the versions it resolved.

use crate::{OverriddenDependencyMatcher, VersionsOverrider};
use node_semver::Range;
use pnpm_catalogs_protocol_parser::parse_catalog_protocol;
use pnpm_lockfile::{
    ImporterDepVersion, Lockfile, PkgName, ProjectSnapshot, ResolvedDependencyMap,
    ResolvedDependencySpec,
};
use pnpm_lockfile_preferred_versions::get_version_selector_type;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_registry::RangeSpecStyle;
use pnpm_resolving_npm_resolver::{calc_version_range, infer_range_spec_style};
use pnpm_resolving_resolver_base::VersionSelectorType;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Mutex,
};

/// Which declared ranges `pacquet update` may move onto the versions the run
/// resolves, and where the answer is reported back.
///
/// A compatible update cannot know the version it will land on before it has
/// resolved: the pick depends on the ranges the workspace's other importers
/// declare, on the overrides, and on the peer graph. So the update names the
/// direct dependencies it targets, the fresh-resolve path rewrites their
/// entries in the lockfile it is about to write, and reports each new range
/// back here for the update to write into `package.json` — or into the
/// catalog entry the dependency points at.
pub struct ManifestSpecBumps {
    /// Per importer id, the direct-dependency aliases whose range may move,
    /// each mapped to the group its `package.json` declares it under and the
    /// specifier declared there. The declaration is what tells a range the
    /// update owns from one an override replaced before the resolver read it:
    /// only a lockfile entry that still carries the declared text is bumped.
    pub targets: BTreeMap<String, HashMap<String, (DependencyGroup, String)>>,
    /// The range operator to write when the declaration pins none.
    pub range_spec_style: RangeSpecStyle,
    /// What the resolve settled on, for the declarations whose text changed.
    pub applied: Mutex<AppliedSpecBumps>,
}

/// The ranges [`ManifestSpecBumps`] moved, split by where they are declared.
#[derive(Debug, Default)]
pub struct AppliedSpecBumps {
    /// Importer id → alias → the group the range is declared under and the
    /// new range. The group travels with the range so the manifest rewrites
    /// the entry the lockfile rewrote, rather than re-deriving it from the
    /// alias and risking a different pick.
    pub manifests: BTreeMap<String, BTreeMap<String, (DependencyGroup, String)>>,
    /// Catalog name → alias → new range, for a dependency declared through
    /// `catalog:`, where the entry owns the range.
    pub catalogs: BTreeMap<String, BTreeMap<String, String>>,
}

impl AppliedSpecBumps {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.manifests.is_empty() && self.catalogs.is_empty()
    }
}

/// The override set an update runs under, paired with the importer manifests
/// whose own name and version a `parent>child` override key is matched
/// against.
///
/// An override governs a declaration even when it repeats it verbatim, and a
/// declaration an override governs is not the update's to move: the hook
/// rewrites the bumped text away before the next resolve reads it, leaving a
/// lockfile specifier the manifest never shows and a `--frozen-lockfile`
/// install that rejects the pair (pnpm/pnpm#14224).
pub(crate) struct OverriddenDeclarations<'a> {
    pub overrider: &'a VersionsOverrider,
    pub importer_manifests: &'a BTreeMap<String, &'a PackageManifest>,
}

impl<'a> OverriddenDeclarations<'a> {
    fn matcher_for(&self, importer_id: &str) -> Option<OverriddenDependencyMatcher<'a>> {
        self.importer_manifests
            .get(importer_id)
            .map(|manifest| self.overrider.dependency_matcher(manifest.value()))
    }
}

/// Rewrite the targeted ranges in `lockfile` — which this run built and has
/// not written yet — around the versions it records, and report them into
/// [`ManifestSpecBumps::applied`].
///
/// Rewriting the lockfile's own copy of each range here is what keeps it
/// agreeing with the `package.json` the update writes afterwards: the two
/// carry the same text, and the version recorded against it satisfies it by
/// construction.
pub(crate) fn apply_manifest_spec_bumps(
    lockfile: &mut Lockfile,
    bumps: &ManifestSpecBumps,
    overridden: Option<&OverriddenDeclarations<'_>>,
) {
    let mut manifests: BTreeMap<String, HashMap<PkgName, (DependencyGroupIndex, String)>> =
        BTreeMap::new();
    let mut cataloged: HashSet<(String, PkgName)> = HashSet::new();

    for (importer_id, targets) in &bumps.targets {
        let Some(importer) = lockfile.importers.get(importer_id) else { continue };
        let override_matcher =
            overridden.and_then(|overridden| overridden.matcher_for(importer_id));
        for (alias, (manifest_group, manifest_specifier)) in targets {
            // An override — or another manifest hook — governs this entry, so
            // the version the run resolved answers the override rather than
            // the declaration. Leaving the lockfile entry and `package.json`
            // alone keeps the two agreeing and keeps the declaration, a
            // `catalog:` reference included (pnpm/pnpm#12115). An override
            // that repeats the declaration verbatim rewrites nothing, which
            // is why the text comparison below cannot stand in for this
            // (pnpm/pnpm#14224).
            if override_matcher
                .as_ref()
                .is_some_and(|matcher| matcher.matches(alias, manifest_specifier))
            {
                continue;
            }
            let Ok(alias) = PkgName::parse(alias.as_str()) else { continue };
            let Some((group, declared)) = declared_dependency(importer, &alias, *manifest_group)
            else {
                continue;
            };
            if declared.specifier != *manifest_specifier {
                continue;
            }
            if let Some(catalog_name) = parse_catalog_protocol(&declared.specifier) {
                cataloged.insert((catalog_name.to_string(), alias));
                continue;
            }
            if let Some(bumped) =
                bumped_range(&declared.specifier, &declared.version, bumps.range_spec_style)
            {
                manifests.entry(importer_id.clone()).or_default().insert(alias, (group, bumped));
            }
        }
    }

    let mut catalogs: BTreeMap<String, HashMap<PkgName, String>> = BTreeMap::new();
    for (catalog_name, alias) in &cataloged {
        let Some(entry) = lockfile
            .catalogs
            .as_ref()
            .and_then(|catalogs| catalogs.get(catalog_name))
            .and_then(|catalog| catalog.get(&alias.to_string()))
        else {
            continue;
        };
        let Ok(version) = entry.version.parse::<ImporterDepVersion>() else { continue };
        let Some(bumped) = bumped_range(&entry.specifier, &version, bumps.range_spec_style) else {
            continue;
        };
        catalogs.entry(catalog_name.clone()).or_default().insert(alias.clone(), bumped);
    }

    for (importer_id, bumped) in &manifests {
        let Some(importer) = lockfile.importers.get_mut(importer_id) else { continue };
        let mut groups = dependency_maps_mut(importer);
        for (alias, (group, specifier)) in bumped {
            if let Some(declared) = groups[*group].as_mut().and_then(|map| map.get_mut(alias)) {
                declared.specifier.clone_from(specifier);
            }
        }
    }
    for (catalog_name, bumped) in &catalogs {
        let Some(catalog) =
            lockfile.catalogs.as_mut().and_then(|catalogs| catalogs.get_mut(catalog_name))
        else {
            continue;
        };
        for (alias, specifier) in bumped {
            if let Some(entry) = catalog.get_mut(&alias.to_string()) {
                entry.specifier.clone_from(specifier);
            }
        }
    }

    let mut applied = bumps.applied.lock().expect("the spec-bump sink is never poisoned");
    applied.manifests =
        render_aliases(manifests, |(group, specifier)| (IMPORTER_GROUPS[group], specifier));
    applied.catalogs = render_aliases(catalogs, |specifier| specifier);
}

fn render_aliases<Bumped, Rendered>(
    bumped: BTreeMap<String, HashMap<PkgName, Bumped>>,
    specifier: impl Fn(Bumped) -> Rendered,
) -> BTreeMap<String, BTreeMap<String, Rendered>> {
    bumped
        .into_iter()
        .map(|(owner, aliases)| {
            let aliases = aliases
                .into_iter()
                .map(|(alias, bumped)| (alias.to_string(), specifier(bumped)))
                .collect();
            (owner, aliases)
        })
        .collect()
}

/// The range that pins `version` for a dependency that currently declares
/// `declared`, or `None` when the declaration is not a range this may move.
///
/// The range text is [`calc_version_range`]'s decision — the same one the
/// npm resolver's `calc_specifier` makes for a version it has just picked.
fn bumped_range(
    declared: &str,
    version: &ImporterDepVersion,
    default_style: RangeSpecStyle,
) -> Option<String> {
    let (prefix, declared_range) = split_npm_alias(declared)?;
    // A dist-tag names no version of its own, so the version behind it
    // moving leaves the declaration saying exactly what was asked for.
    if get_version_selector_type(declared_range) == Some(VersionSelectorType::Tag) {
        return None;
    }
    let resolved = match version {
        ImporterDepVersion::Regular(version) => version.version_semver()?,
        ImporterDepVersion::Alias(alias) => alias.suffix.version_semver()?,
        // A link or an injected directory has no version to pin.
        ImporterDepVersion::Link(_) | ImporterDepVersion::File(_) => return None,
    };
    let range =
        calc_version_range(resolved, infer_range_spec_style(declared_range), None, default_style);
    let bumped = format!("{prefix}{range}");
    (bumped != declared).then_some(bumped)
}

/// A declared specifier split into the `npm:<name>@` prefix it keeps and the
/// range behind it. `None` for any other protocol — a `workspace:`, `link:`,
/// `file:`, git, tarball or named-registry dependency declares no registry
/// range to move.
fn split_npm_alias(declared: &str) -> Option<(&str, &str)> {
    let Some(rest) = declared.strip_prefix("npm:") else {
        return (!declared.contains(':')).then_some(("", declared));
    };
    // A bare `npm:<range>` names no other package, so it round-trips as one.
    if rest.parse::<Range>().is_ok() {
        return Some(("npm:", rest));
    }
    let at = rest.rfind('@').filter(|index| *index >= 1)?;
    let prefix_len = declared.len() - rest.len() + at + 1;
    Some((&declared[..prefix_len], &declared[prefix_len..]))
}

/// Index into an importer's dependency maps, in the order
/// [`declared_dependency`] and [`dependency_maps_mut`] both lay them out.
/// Carried so a dependency declared in more than one group only has the
/// range that was read rewritten.
type DependencyGroupIndex = usize;

/// The manifest group each of an importer's dependency maps is built from,
/// in [`DependencyGroupIndex`] order.
const IMPORTER_GROUPS: [DependencyGroup; 3] =
    [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional];

/// The importer's entry for `alias` under the group the manifest declares it
/// in. Anchoring on that group rather than on the first map that happens to
/// carry the alias keeps a dependency declared in more than one group reading
/// and rewriting the same entry.
fn declared_dependency<'a>(
    importer: &'a ProjectSnapshot,
    alias: &PkgName,
    group: DependencyGroup,
) -> Option<(DependencyGroupIndex, &'a ResolvedDependencySpec)> {
    let index = IMPORTER_GROUPS.iter().position(|candidate| *candidate == group)?;
    let maps =
        [&importer.dependencies, &importer.dev_dependencies, &importer.optional_dependencies];
    Some((index, maps[index].as_ref()?.get(alias)?))
}

fn dependency_maps_mut(importer: &mut ProjectSnapshot) -> [Option<&mut ResolvedDependencyMap>; 3] {
    [
        importer.dependencies.as_mut(),
        importer.dev_dependencies.as_mut(),
        importer.optional_dependencies.as_mut(),
    ]
}

#[cfg(test)]
mod tests;
