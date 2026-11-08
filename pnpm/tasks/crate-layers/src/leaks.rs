use crate::{
    dependencies::Reach,
    modules::CrateModules,
    surface::{self, Surface},
    use_tree::UsePath,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
};

/// A workspace crate whose public signatures name items of one of its
/// dependencies without re-exporting them, so a caller that uses those items
/// has to depend on that dependency too.
#[derive(Debug, PartialEq, Eq)]
pub struct Leak {
    pub crate_name: String,
    pub dependency: String,
    /// The leaked paths, such as `pnpm_http::Client`.
    pub items: BTreeSet<String>,
    /// The crates that depend on both `crate_name` and `dependency`.
    pub shared_dependents: BTreeSet<String>,
}

/// Every leak in the workspace, most shared dependents first.
pub fn find(reach: &Reach, modules: &BTreeMap<String, CrateModules>) -> Vec<Leak> {
    let mut leaks = Vec::new();
    for (crate_name, modules) in modules {
        let (exposed, reexported) = dealiased(modules);
        for dependency in reach.direct
            .get(crate_name)
            .into_iter()
            .flatten()
        {
            let items = leaked_items(&exposed, &reexported, &lib_name(dependency));
            if items.is_empty() {
                continue;
            }
            leaks.push(Leak {
                crate_name: crate_name.clone(),
                dependency: dependency.clone(),
                items,
                shared_dependents: shared_dependents(reach, crate_name, dependency),
            });
        }
    }
    leaks.sort_by_key(|leak| {
        let key = (leak.crate_name.clone(), leak.dependency.clone());
        (std::cmp::Reverse(leak.shared_dependents.len()), key)
    });
    leaks
}

/// The crates that depend directly on both `first` and `second`.
fn shared_dependents(reach: &Reach, first: &str, second: &str) -> BTreeSet<String> {
    reach.direct
        .iter()
        .filter(|(_, deps)| deps.contains(first) && deps.contains(second))
        .map(|(name, _)| name.clone())
        .collect()
}

/// The number of dependencies each crate leaks, for [`crate::ratchet::check`].
pub fn counts(leaks: &[Leak]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for leak in leaks {
        *counts.entry(leak.crate_name.clone()).or_default() += 1;
    }
    counts
}

/// The exposed and re-exported paths of every module, with an `extern crate`
/// alias replaced by the crate's name. A module sees its own aliases first,
/// then the crate root's.
fn dealiased(modules: &CrateModules) -> (Vec<Vec<String>>, Vec<Vec<String>>) {
    let root_aliases = modules
        .root_surface()
        .map(|root| root.crate_aliases.as_slice())
        .unwrap_or_default();
    let mut exposed = Vec::new();
    let mut reexported = Vec::new();
    for module in modules.surfaces() {
        let aliases = module_aliases(module, root_aliases);
        exposed.extend(module.exposed.iter().map(|path| surface::dealias(path, &aliases)));
        exposed.extend(module.resolved.exposed.iter().cloned());
        let reexports = module.reexported.iter();
        reexported.extend(reexports.map(|reexport| surface::dealias(&reexport.segments, &aliases)));
        reexported.extend(module.resolved.reexported.iter().cloned());
    }
    (exposed, reexported)
}

fn module_aliases<'surface>(
    module: &'surface Surface,
    root_aliases: &'surface [UsePath],
) -> Vec<&'surface UsePath> {
    module.crate_aliases
        .iter()
        .chain(root_aliases)
        .collect()
}

/// The paths into `lib` that `exposed` names and `reexported` does not
/// cover. A re-export covers the item itself, or a module or glob that
/// contains it.
fn leaked_items(
    exposed: &[Vec<String>],
    reexported: &[Vec<String>],
    lib: &str,
) -> BTreeSet<String> {
    let covered = |path: &[String]| {
        reexported
            .iter()
            .any(|reexport| !reexport.is_empty() && path.starts_with(reexport))
    };
    exposed
        .iter()
        .filter(|path| path.len() > 1 && path[0] == lib && !covered(path))
        .map(|path| path.join("::"))
        .collect()
}

/// The name a crate is imported by in Rust source.
fn lib_name(crate_name: &str) -> String {
    crate_name.replace('-', "_")
}

/// One line per leak: the crates, the shared dependents, and the items.
pub fn render_summary(leaks: &[Leak]) -> String {
    let mut text = String::from("crate | leaks | shared dependents | items\n");
    for leak in leaks {
        let _ = writeln!(
            text,
            "{} | {} | {} | {}",
            leak.crate_name,
            leak.dependency,
            leak.shared_dependents.len(),
            leak.items.len(),
        );
    }
    text
}

/// One crate's leaks, with the leaked items and the shared dependents.
pub fn render_crate(leaks: &[Leak], crate_name: &str) -> String {
    let own = leaks
        .iter()
        .filter(|leak| leak.crate_name == crate_name)
        .collect::<Vec<_>>();
    if own.is_empty() {
        return format!("{crate_name} names no dependency's items in its public signatures.\n");
    }
    let mut text = String::new();
    for leak in own {
        let _ = writeln!(text, "{crate_name} leaks {}:", leak.dependency);
        push_list(&mut text, "Items:", &leak.items);
        push_list(&mut text, "Shared dependents:", &leak.shared_dependents);
    }
    text
}

fn push_list(text: &mut String, heading: &str, names: &BTreeSet<String>) {
    let _ = writeln!(text, "  {heading}");
    for name in names {
        let _ = writeln!(text, "    {name}");
    }
}
