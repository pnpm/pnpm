use crate::{dependencies::Reach, modules::CrateModules, surface::Surface, use_tree::UsePath};
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
        let surfaces = modules.surfaces().collect::<Vec<_>>();
        let exposed = exposed_paths(&surfaces);
        for dependency in reach.direct
            .get(crate_name)
            .into_iter()
            .flatten()
        {
            let items = leaked_items(&surfaces, &exposed, &lib_name(dependency));
            if items.is_empty() {
                continue;
            }
            let shared_dependents = reach.direct
                .iter()
                .filter(|(_, deps)| deps.contains(crate_name) && deps.contains(dependency))
                .map(|(name, _)| name.clone())
                .collect();
            leaks.push(Leak {
                crate_name: crate_name.clone(),
                dependency: dependency.clone(),
                items,
                shared_dependents,
            });
        }
    }
    leaks.sort_by_key(|leak| {
        let key = (leak.crate_name.clone(), leak.dependency.clone());
        (std::cmp::Reverse(leak.shared_dependents.len()), key)
    });
    leaks
}

/// The number of dependencies each crate leaks, for [`crate::ratchet::check`].
pub fn counts(leaks: &[Leak]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for leak in leaks {
        *counts.entry(leak.crate_name.clone()).or_default() += 1;
    }
    counts
}

/// The exposed paths of every surface, with an `extern crate` alias replaced
/// by the crate's name.
fn exposed_paths(surfaces: &[&Surface]) -> Vec<Vec<String>> {
    let aliases = surfaces
        .iter()
        .flat_map(|surface| &surface.crate_aliases)
        .collect::<Vec<_>>();
    let mut exposed = surfaces
        .iter()
        .flat_map(|surface| surface.exposed.iter().cloned())
        .collect::<Vec<_>>();
    for path in &mut exposed {
        let alias = aliases
            .iter()
            .find(|alias| alias.binding.as_ref() == path.first());
        if let Some(alias) = alias {
            path.splice(0..1, alias.segments.iter().cloned());
        }
    }
    exposed
}

/// The paths into `lib` that `exposed` names and the surfaces do not re-export.
fn leaked_items(surfaces: &[&Surface], exposed: &[Vec<String>], lib: &str) -> BTreeSet<String> {
    let reexports = surfaces
        .iter()
        .flat_map(|surface| &surface.reexported)
        .filter(|reexport| {
            reexport.segments
                .first()
                .is_some_and(|first| first == lib)
        })
        .collect::<Vec<_>>();
    exposed
        .iter()
        .filter(|path| path.len() > 1 && path[0] == lib)
        .filter(|path| !reexports.iter().any(|reexport| covers(reexport, path)))
        .map(|path| path.join("::"))
        .collect()
}

/// Whether a `pub use` makes `path` reachable through the crate: it
/// re-exports the item itself, or a module or glob that contains it.
fn covers(reexport: &UsePath, path: &[String]) -> bool {
    path.starts_with(&reexport.segments)
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
