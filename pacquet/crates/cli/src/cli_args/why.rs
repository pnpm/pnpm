//! `pacquet why` — show the packages that depend on `<pkg>`.
//!
//! Ports pnpm's
//! [`why` command](https://github.com/pnpm/pnpm/blob/deps/inspection/commands/src/listing/why.ts)
//! and the reverse-tree builder in
//! [`buildDependentsTree`](https://github.com/pnpm/pnpm/blob/deps/inspection/tree-builder/src/buildDependentsTree.ts).

use crate::State;
use clap::Args;
use owo_colors::{OwoColorize, Stream};
use pacquet_config::matcher::{create_matcher, Matcher};
use pacquet_lockfile::{Lockfile, PkgName, PkgNameVerPeer, PkgVerPeer};
use pacquet_package_manifest::DependencyGroup;
use std::{collections::{HashMap, HashSet}, io::Write};

#[derive(Debug)]
struct DependentNode {
    name: String,
    version: String,
    dep_field: Option<DependencyGroup>,
    dependents: Vec<DependentNode>,
}

#[derive(Debug)]
struct WhyResult {
    name: String,
    version: String,
    dependents: Vec<DependentNode>,
}

#[derive(Debug, Args)]
pub struct WhyArgs {
    pub packages: Vec<String>,

    #[clap(long)]
    pub depth: Option<usize>,
}

impl WhyArgs {
    pub async fn run(self, state: State) -> miette::Result<()> {
        if self.packages.is_empty() {
            return Err(miette::miette!(
                code = "ERR_PNPM_MISSING_PACKAGE_NAME",
                "`pacquet why` requires a package name or pattern"
            ));
        }

        let lockfile = state
            .lockfile
            .get()
            .map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?;

        let Some(lockfile) = lockfile else {
            let dir =
                state.manifest.path().parent().unwrap_or_else(|| state.manifest.path()).display();
            return Err(miette::miette!(
                code = "ERR_PNPM_OUTDATED_NO_LOCKFILE",
                "No lockfile in directory \"{dir}\". Run `pacquet install` to generate one."
            ));
        };

        let matcher = create_matcher(&self.packages);
        let results = build_dependents_tree(lockfile, &matcher);

        if results.is_empty() {
            return Ok(());
        }

        let output = render_tree(&results, self.depth);
        let mut stdout = std::io::stdout();
        let _ = writeln!(stdout, "{output}");
        let _ = stdout.flush();

        Ok(())
    }
}

const MAX_REVERSE_WALK_DEPTH: usize = 64;

fn build_dependents_tree(lockfile: &Lockfile, matcher: &Matcher) -> Vec<WhyResult> {
    let Some(packages) = lockfile.packages.as_ref() else {
        return vec![];
    };

    let snapshots = lockfile.snapshots.as_ref();

    let mut forward_edges: HashMap<PkgNameVerPeer, Vec<(String, Option<PkgNameVerPeer>)>> =
        HashMap::new();

    for key in packages.keys() {
        let Some(snapshot) = snapshots.and_then(|s| s.get(key)) else {
            continue;
        };

        let mut edges: Vec<(String, Option<PkgNameVerPeer>)> = Vec::new();

        if let Some(deps) = &snapshot.dependencies {
            for (alias, dep_ref) in deps {
                edges.push((alias.to_string(), dep_ref.resolve(alias)));
            }
        }

        if let Some(optional_deps) = &snapshot.optional_dependencies {
            for (alias, dep_ref) in optional_deps {
                edges.push((alias.to_string(), dep_ref.resolve(alias)));
            }
        }

        forward_edges.insert(key.clone(), edges);
    }

    let mut reverse_map: HashMap<PkgNameVerPeer, Vec<(PkgNameVerPeer, String)>> = HashMap::new();

    if let Some(importer) = lockfile.root_project() {
        let groups = [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional];
        for group in groups {
            if let Some(deps) = importer.get_map_by_group(group) {
                for (alias, spec) in deps {
                    if let Some(target_key) = spec.version.resolved_key(alias) {
                        reverse_map.entry(target_key).or_default().push((
                            PkgNameVerPeer::new(
                                PkgName::parse(".").unwrap(),
                                "0.0.0".parse::<PkgVerPeer>().unwrap(),
                            ),
                            alias.to_string(),
                        ));
                    }
                }
            }
        }
    }

    for (parent_key, edges) in &forward_edges {
        for (alias, target) in edges {
            if let Some(target_key) = target {
                reverse_map
                    .entry(target_key.clone())
                    .or_default()
                    .push((parent_key.clone(), alias.clone()));
            }
        }
    }

    for edges in reverse_map.values_mut() {
        edges.sort_by_key(|a| a.0.to_string());
    }

    let mut results: Vec<WhyResult> = Vec::new();

    for key in packages.keys() {
        let name = key.name.to_string();
        let version = key.suffix.version().to_string();

        let matched = matcher.matches(&name)
            || reverse_map
                .get(key)
                .is_some_and(|edges| edges.iter().any(|(_parent, alias)| matcher.matches(alias)));

        if !matched {
            continue;
        }

        let dependents = walk_reverse(key, &reverse_map, &mut HashSet::new(), 0);

        results.push(WhyResult { name: name.clone(), version, dependents });
    }

    results.sort_by(|a, b| a.name.cmp(&b.name).then(a.version.cmp(&b.version)));

    results
}

fn walk_reverse(
    node_key: &PkgNameVerPeer,
    reverse_map: &HashMap<PkgNameVerPeer, Vec<(PkgNameVerPeer, String)>>,
    visited: &mut HashSet<PkgNameVerPeer>,
    depth: usize,
) -> Vec<DependentNode> {
    if depth >= MAX_REVERSE_WALK_DEPTH {
        return vec![];
    }

    let Some(edges) = reverse_map.get(node_key) else {
        return vec![];
    };

    let mut dependents = Vec::new();

    for (parent_key, _alias) in edges {
        let is_root_importer = parent_key.name.scope.is_none() && parent_key.name.bare == ".";

        if is_root_importer {
            dependents.push(DependentNode {
                name: "project".to_string(),
                version: "0.0.0".to_string(),
                dep_field: None,
                dependents: vec![],
            });
            continue;
        }

        if visited.contains(parent_key) {
            dependents.push(DependentNode {
                name: parent_key.name.to_string(),
                version: parent_key.suffix.version().to_string(),
                dep_field: None,
                dependents: vec![],
            });
            continue;
        }

        visited.insert(parent_key.clone());

        let parent_name = parent_key.name.to_string();
        let parent_version = parent_key.suffix.version().to_string();

        let child_dependents = walk_reverse(parent_key, reverse_map, visited, depth + 1);

        visited.remove(parent_key);

        dependents.push(DependentNode {
            name: parent_name,
            version: parent_version,
            dep_field: None,
            dependents: child_dependents,
        });
    }

    dependents
}

fn render_tree(results: &[WhyResult], max_depth: Option<usize>) -> String {
    let mut output = String::new();

    for (i, result) in results.iter().enumerate() {
        if i > 0 {
            output.push_str("\n\n");
        }

        let root_label = format!("{}{}", bold(&result.name), dim(&format!("@{}", result.version)));
        output.push_str(&root_label);

        if result.dependents.is_empty() {
            continue;
        }

        output.push('\n');
        render_dependents(&mut output, &result.dependents, "", max_depth, 0);
    }

    output
}

fn render_dependents(
    output: &mut String,
    dependents: &[DependentNode],
    prefix: &str,
    max_depth: Option<usize>,
    current_depth: usize,
) {
    if let Some(max) = max_depth
        && current_depth >= max
    {
        return;
    }

    for (i, dep) in dependents.iter().enumerate() {
        let is_last = i == dependents.len() - 1;
        let connector = if is_last { "└── " } else { "├── " };
        let child_prefix = if is_last { "    " } else { "│   " };

        let label = format!(
            "{}{}{}",
            bold(&dep.name),
            dim(&format!("@{}", dep.version)),
            dep.dep_field
                .map(|f| format!(" {}", dim(&format!("({})", dep_field_name(f)))))
                .unwrap_or_default(),
        );

        output.push_str(prefix);
        output.push_str(connector);
        output.push_str(&label);
        output.push('\n');

        if !dep.dependents.is_empty() {
            let new_prefix = format!("{prefix}{child_prefix}");
            render_dependents(output, &dep.dependents, &new_prefix, max_depth, current_depth + 1);
        }
    }
}

fn dep_field_name(field: DependencyGroup) -> &'static str {
    match field {
        DependencyGroup::Prod => "dependencies",
        DependencyGroup::Dev => "devDependencies",
        DependencyGroup::Optional => "optionalDependencies",
        DependencyGroup::Peer => "peerDependencies",
    }
}

fn bold(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |t| t.bold()).to_string()
}

fn dim(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |t| t.dimmed()).to_string()
}

#[cfg(test)]
mod tests;
