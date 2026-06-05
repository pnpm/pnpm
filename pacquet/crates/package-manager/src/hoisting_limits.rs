use pacquet_config::HoistingLimits;
use pacquet_lockfile::{Lockfile, ProjectSnapshot, ResolvedDependencyMap};
use pacquet_real_hoist::percent_encode_path;
use std::collections::{BTreeSet, HashMap};

/// Translate the user-facing [`HoistingLimits`] mode into the
/// `@yarnpkg/nm` hoister's per-locator border map (the shape
/// [`pacquet_real_hoist::HoistOpts::hoisting_limits`] consumes). A
/// name in a locator's set is a hoisting border: that node's
/// dependencies are not hoisted above it.
///
/// Ports pnpm's
/// [`getHoistingLimits`](https://github.com/pnpm/pnpm/blob/89812a9353/installing/linking/real-hoist/src/index.ts):
///
/// - [`HoistingLimits::None`] → empty map (hoist as far as possible).
/// - [`HoistingLimits::Workspaces`] → border every workspace package
///   (and the root's direct deps) at the root locator, so each
///   project's dependencies stay within that project.
/// - [`HoistingLimits::Dependencies`] → additionally border each
///   workspace package's own direct dependencies, so their
///   transitives stay nested beneath them.
///
/// Pacquet's hoister currently hoists into the single root importer
/// only, so it consults the `.@` entry; the per-importer entries the
/// `dependencies` mode emits are produced for parity and become
/// load-bearing once multi-level hoisting lands.
#[must_use]
pub fn get_hoisting_limits(
    importers: &HashMap<String, ProjectSnapshot>,
    mode: HoistingLimits,
) -> pacquet_real_hoist::HoistingLimits {
    let mut limits = pacquet_real_hoist::HoistingLimits::new();
    if matches!(mode, HoistingLimits::None) {
        return limits;
    }

    // The root border accumulates the root's own direct deps plus
    // every (encoded) non-root importer id, regardless of iteration
    // order — `BTreeSet` makes the result deterministic even though
    // `importers` is a `HashMap`. Only stored under `.@` when a root
    // importer is present, matching upstream.
    let mut root_border: BTreeSet<String> = BTreeSet::new();
    let mut root_present = false;

    for (importer_id, importer) in importers {
        if importer_id == Lockfile::ROOT_IMPORTER_KEY {
            root_present = true;
            collect_direct_dep_names(importer, &mut root_border);
            continue;
        }

        root_border.insert(percent_encode_path(importer_id));
        if !matches!(mode, HoistingLimits::Dependencies) {
            // `workspaces` mode borders each package at the root only;
            // their own direct deps don't get a per-importer border.
            continue;
        }

        let mut importer_border: BTreeSet<String> = BTreeSet::new();
        collect_direct_dep_names(importer, &mut importer_border);
        limits.insert(
            format!("{}@workspace:{importer_id}", percent_encode_path(importer_id)),
            importer_border,
        );
    }

    if root_present {
        limits.insert(format!("{}@", Lockfile::ROOT_IMPORTER_KEY), root_border);
    }

    limits
}

/// Collect every direct-dependency alias of `importer` (across the
/// regular, dev, and optional groups) into `out`. Aliases are the
/// node names the hoister matches borders against.
fn collect_direct_dep_names(importer: &ProjectSnapshot, out: &mut BTreeSet<String>) {
    for group in [
        importer.dependencies.as_ref(),
        importer.dev_dependencies.as_ref(),
        importer.optional_dependencies.as_ref(),
    ] {
        let Some(deps) = group else { continue };
        add_alias_names(deps, out);
    }
}

fn add_alias_names(deps: &ResolvedDependencyMap, out: &mut BTreeSet<String>) {
    for alias in deps.keys() {
        out.insert(alias.to_string());
    }
}

#[cfg(test)]
mod tests;
