use crate::PackageExtender;
use indexmap::IndexMap;
use pacquet_config::{PackageExtension, PeerDependencyMeta};
use std::{collections::BTreeMap, sync::LazyLock};

static COMPAT_PACKAGE_EXTENSIONS: LazyLock<IndexMap<String, PackageExtension>> =
    LazyLock::new(|| {
        let entries: Vec<(String, PackageExtension)> =
            serde_json::from_str(include_str!("compat_package_extensions.json"))
                .expect("@yarnpkg/extensions compatibility DB JSON is valid");
        let mut extensions = IndexMap::new();
        for (selector, extension) in entries {
            merge_package_extension_entry(&mut extensions, selector, extension);
        }
        extensions
    });

static COMPAT_PACKAGE_EXTENDER: LazyLock<PackageExtender> = LazyLock::new(|| {
    PackageExtender::new(&COMPAT_PACKAGE_EXTENSIONS)
        .expect("@yarnpkg/extensions compatibility DB selectors are valid")
});

pub(crate) fn compat_package_extender() -> &'static PackageExtender {
    &COMPAT_PACKAGE_EXTENDER
}

fn merge_package_extension_entry(
    extensions: &mut IndexMap<String, PackageExtension>,
    selector: String,
    extension: PackageExtension,
) {
    match extensions.get_mut(&selector) {
        Some(previous) => merge_package_extension(previous, &extension),
        None => {
            extensions.insert(selector, extension);
        }
    }
}

fn merge_package_extension(previous: &mut PackageExtension, next: &PackageExtension) {
    merge_string_map(&mut previous.dependencies, next.dependencies.as_ref());
    merge_string_map(&mut previous.optional_dependencies, next.optional_dependencies.as_ref());
    merge_string_map(&mut previous.peer_dependencies, next.peer_dependencies.as_ref());
    merge_peer_meta_map(&mut previous.peer_dependencies_meta, next.peer_dependencies_meta.as_ref());
}

fn merge_string_map(
    previous: &mut Option<BTreeMap<String, String>>,
    next: Option<&BTreeMap<String, String>>,
) {
    let Some(next) = next else { return };
    let mut merged = next.clone();
    if let Some(previous) = previous.take() {
        merged.extend(previous);
    }
    *previous = Some(merged);
}

fn merge_peer_meta_map(
    previous: &mut Option<BTreeMap<String, PeerDependencyMeta>>,
    next: Option<&BTreeMap<String, PeerDependencyMeta>>,
) {
    let Some(next) = next else { return };
    let mut merged = next.clone();
    if let Some(previous) = previous.take() {
        merged.extend(previous);
    }
    *previous = Some(merged);
}
