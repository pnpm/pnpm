use std::collections::BTreeSet;
use toml::{
    Table,
    Value,
};

pub(super) fn prune_feature_references(manifest: &mut Table, paths: &mut Vec<Vec<String>>) {
    let aliases = paths
        .iter()
        .filter_map(|path| path.last())
        .cloned()
        .collect();
    let dependencies = FeatureDependencies::remaining(manifest);
    let Some(features) = manifest.get_mut("features").and_then(Value::as_table_mut) else {
        return;
    };
    let local = features.keys().cloned().collect();
    for (name, value) in features.iter_mut() {
        let Some(items) = value.as_array_mut() else { continue };
        let count = items.len();
        items.retain(|item| {
            item.as_str()
                .is_none_or(|reference| dependencies.keep(reference, &aliases, &local))
        });
        if items.len() != count {
            paths.push(vec!["features".to_owned(), name.clone()]);
        }
    }
}

#[derive(Default)]
struct FeatureDependencies {
    all: BTreeSet<String>,
    optional: BTreeSet<String>,
}

impl FeatureDependencies {
    fn remaining(manifest: &Table) -> Self {
        let mut dependencies = Self::default();
        dependencies.collect(manifest);
        if let Some(targets) = manifest.get("target").and_then(Value::as_table) {
            for table in targets.values().filter_map(Value::as_table) {
                dependencies.collect(table);
            }
        }
        dependencies
    }

    fn collect(&mut self, table: &Table) {
        for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
            let Some(entries) = table.get(section).and_then(Value::as_table) else { continue };
            for (alias, spec) in entries {
                self.all.insert(alias.clone());
                if spec
                    .get("optional")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    self.optional.insert(alias.clone());
                }
            }
        }
    }

    fn keep(&self, reference: &str, aliases: &BTreeSet<String>, local: &BTreeSet<String>) -> bool {
        let (alias, keep) = if let Some(alias) = reference.strip_prefix("dep:") {
            (alias, self.optional.contains(alias))
        } else if let Some((alias, _)) = reference.split_once("?/") {
            (alias, self.optional.contains(alias))
        } else if let Some((alias, _)) = reference.split_once('/') {
            (alias, self.all.contains(alias))
        } else {
            (reference, local.contains(reference) || self.optional.contains(reference))
        };
        keep || !aliases.contains(alias)
    }
}
