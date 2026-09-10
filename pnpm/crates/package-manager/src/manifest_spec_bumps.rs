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
    borrow::Cow,
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
    let (manifests, cataloged) = collect_importer_bumps(lockfile, bumps, overridden);
    let catalogs = collect_catalog_bumps(lockfile, &cataloged, bumps.range_spec_style);
    apply_importer_bumps(lockfile, &manifests);
    apply_catalog_bumps(lockfile, &catalogs);

    let mut applied = bumps.applied.lock().expect("the spec-bump sink is never poisoned");
    applied.manifests =
        render_aliases(manifests, |(group, specifier)| (IMPORTER_GROUPS[group], specifier));
    applied.catalogs = render_aliases(catalogs, |specifier| specifier);
}

/// The ranges every importer's declarations move to, plus the catalog entries
/// a `catalog:` declaration defers the move to.
fn collect_importer_bumps(
    lockfile: &Lockfile,
    bumps: &ManifestSpecBumps,
    overridden: Option<&OverriddenDeclarations<'_>>,
) -> (ImporterBumps, HashSet<(String, PkgName)>) {
    let mut manifests: ImporterBumps = BTreeMap::new();
    let mut cataloged: HashSet<(String, PkgName)> = HashSet::new();
    for (importer_id, targets) in &bumps.targets {
        let Some(importer) = lockfile.importers.get(importer_id) else { continue };
        let override_matcher =
            overridden.and_then(|overridden| overridden.matcher_for(importer_id));
        for (alias, (manifest_group, manifest_specifier)) in targets {
            let target = SpecBumpTarget {
                importer,
                override_matcher: override_matcher.as_ref(),
                alias,
                manifest_group: *manifest_group,
                manifest_specifier,
                range_spec_style: bumps.range_spec_style,
            };
            record_spec_bump(&mut manifests, &mut cataloged, importer_id, spec_bump(&target));
        }
    }
    (manifests, cataloged)
}

fn record_spec_bump(
    manifests: &mut ImporterBumps,
    cataloged: &mut HashSet<(String, PkgName)>,
    importer_id: &str,
    bump: SpecBump,
) {
    match bump {
        SpecBump::Skip => {}
        SpecBump::Cataloged { catalog_name, alias } => {
            cataloged.insert((catalog_name, alias));
        }
        SpecBump::Manifest { alias, group, bumped } => {
            manifests.entry(importer_id.to_string()).or_default().insert(alias, (group, bumped));
        }
    }
}

/// Per importer id, the bumped range of each declaration and the group it is
/// declared under.
type ImporterBumps = BTreeMap<String, HashMap<PkgName, (DependencyGroupIndex, String)>>;

/// One targeted declaration and what decides whether its range may move.
struct SpecBumpTarget<'a> {
    importer: &'a ProjectSnapshot,
    override_matcher: Option<&'a OverriddenDependencyMatcher<'a>>,
    alias: &'a str,
    manifest_group: DependencyGroup,
    manifest_specifier: &'a str,
    range_spec_style: RangeSpecStyle,
}

/// Where one declaration's bumped range is written.
enum SpecBump {
    /// The declaration keeps its text.
    Skip,
    /// The declaration is a `catalog:` reference, so the catalog entry moves.
    Cataloged {
        catalog_name: String,
        alias: PkgName,
    },
    Manifest {
        alias: PkgName,
        group: DependencyGroupIndex,
        bumped: String,
    },
}

fn spec_bump(target: &SpecBumpTarget<'_>) -> SpecBump {
    // An override — or another manifest hook — governs this entry, so
    // the version the run resolved answers the override rather than
    // the declaration. Leaving the lockfile entry and `package.json`
    // alone keeps the two agreeing and keeps the declaration, a
    // `catalog:` reference included (pnpm/pnpm#12115). An override
    // that repeats the declaration verbatim rewrites nothing, which
    // is why the text comparison below cannot stand in for this
    // (pnpm/pnpm#14224).
    if target
        .override_matcher
        .is_some_and(|matcher| matcher.matches(target.alias, target.manifest_specifier))
    {
        return SpecBump::Skip;
    }
    let Ok(alias) = PkgName::parse(target.alias) else { return SpecBump::Skip };
    let Some((group, declared)) =
        declared_dependency(target.importer, &alias, target.manifest_group)
    else {
        return SpecBump::Skip;
    };
    if declared.specifier != target.manifest_specifier {
        return SpecBump::Skip;
    }
    if let Some(catalog_name) = parse_catalog_protocol(&declared.specifier) {
        return SpecBump::Cataloged { catalog_name: catalog_name.to_string(), alias };
    }
    let Some(bumped) =
        bumped_range(&declared.specifier, &declared.version, target.range_spec_style)
    else {
        return SpecBump::Skip;
    };
    SpecBump::Manifest { alias, group, bumped }
}

fn collect_catalog_bumps(
    lockfile: &Lockfile,
    cataloged: &HashSet<(String, PkgName)>,
    range_spec_style: RangeSpecStyle,
) -> BTreeMap<String, HashMap<PkgName, String>> {
    let mut catalogs: BTreeMap<String, HashMap<PkgName, String>> = BTreeMap::new();
    for (catalog_name, alias) in cataloged {
        let Some(entry) = lockfile
            .catalogs
            .as_ref()
            .and_then(|catalogs| catalogs.get(catalog_name))
            .and_then(|catalog| catalog.get(&alias.to_string()))
        else {
            continue;
        };
        let Ok(version) = entry.version.parse::<ImporterDepVersion>() else { continue };
        let Some(bumped) = bumped_range(&entry.specifier, &version, range_spec_style) else {
            continue;
        };
        catalogs.entry(catalog_name.clone()).or_default().insert(alias.clone(), bumped);
    }
    catalogs
}

fn apply_importer_bumps(lockfile: &mut Lockfile, manifests: &ImporterBumps) {
    for (importer_id, bumped) in manifests {
        let Some(importer) = lockfile.importers.get_mut(importer_id) else { continue };
        let mut groups = dependency_maps_mut(importer);
        for (alias, (group, specifier)) in bumped {
            if let Some(declared) = groups[*group].as_mut().and_then(|map| map.get_mut(alias)) {
                declared.specifier.clone_from(specifier);
            }
        }
    }
}

fn apply_catalog_bumps(
    lockfile: &mut Lockfile,
    catalogs: &BTreeMap<String, HashMap<PkgName, String>>,
) {
    for (catalog_name, bumped) in catalogs {
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
    let (prefix, declared_range) = split_registry_alias(declared)?;
    // A dist-tag names no version of its own, so the version behind it
    // moving leaves the declaration saying exactly what was asked for. A
    // declaration naming only the package tracks the default tag the same
    // way.
    if declared_range.is_empty()
        || get_version_selector_type(declared_range) == Some(VersionSelectorType::Tag)
    {
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

/// A declared specifier split into the `npm:<name>@` or `jsr:<name>@`
/// prefix it keeps and the range behind it. A declaration naming only the
/// package (`jsr:@scope/pkg`, `npm:foo`) keeps the whole name as its prefix
/// and declares an empty range. `None` for any other protocol — a
/// `workspace:`, `link:`, `file:`, git, tarball or named-registry
/// dependency declares no registry range to move.
pub(crate) fn split_registry_alias(declared: &str) -> Option<(Cow<'_, str>, &str)> {
    let Some((protocol, rest)) = ["npm:", "jsr:"]
        .into_iter()
        .find_map(|protocol| declared.strip_prefix(protocol).map(|rest| (protocol, rest)))
    else {
        return (!declared.contains(':')).then_some((Cow::Borrowed(""), declared));
    };
    // A bare `npm:<range>` or `jsr:<range>` names no other package, so it
    // round-trips as one.
    if rest.parse::<Range>().is_ok() {
        return Some((Cow::Borrowed(protocol), rest));
    }
    match rest.rfind('@').filter(|index| *index >= 1) {
        Some(at) => {
            let prefix_len = protocol.len() + at + 1;
            Some((Cow::Borrowed(&declared[..prefix_len]), &declared[prefix_len..]))
        }
        None => Some((Cow::Owned(format!("{declared}@")), "")),
    }
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
