use super::{
    BTreeMap,
    BTreeSet,
    Catalogs,
    parse_catalog_protocol,
};

pub(super) fn find_changed_catalog_entries(
    before: &Catalogs,
    after: &Catalogs,
) -> BTreeMap<String, BTreeSet<String>> {
    before
        .keys()
        .chain(after.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|catalog_name| {
            let dependency_names: BTreeSet<_> = before
                .get(catalog_name)
                .into_iter()
                .flatten()
                .map(|(name, _)| name)
                .chain(
                    after
                        .get(catalog_name)
                        .into_iter()
                        .flatten()
                        .map(|(name, _)| name),
                )
                .collect();
            let changed = dependency_names
                .into_iter()
                .filter(|dependency_name| {
                    before
                        .get(catalog_name)
                        .and_then(|catalog| catalog.get(*dependency_name))
                        != after
                            .get(catalog_name)
                            .and_then(|catalog| catalog.get(*dependency_name))
                })
                .cloned()
                .collect::<BTreeSet<_>>();
            (!changed.is_empty()).then(|| (catalog_name.clone(), changed))
        })
        .collect()
}

pub(super) fn uses_changed_catalog_entry<'a>(
    dependency_groups: impl IntoIterator<Item = Option<&'a BTreeMap<String, String>>>,
    changed_catalog_entries: &BTreeMap<String, BTreeSet<String>>,
) -> bool {
    for dependencies in dependency_groups {
        let Some(dependencies) = dependencies else {
            continue;
        };
        for (dependency_name, spec) in dependencies {
            let Some(catalog_name) = parse_catalog_protocol(spec) else {
                continue;
            };
            let Some(names) = changed_catalog_entries.get(catalog_name) else {
                continue;
            };
            if names.contains(dependency_name) {
                return true;
            }
        }
    }
    false
}
