use super::resolve::{
    ResolvedEntry,
    fallback_version,
};
use crate::ImporterDependencyGraph;
use pnpm_lockfile::{
    ImporterDepVersion,
    PackageKey,
    PackageMetadata,
};
use pnpm_package_manifest::DependencyGroup;
use pnpm_reporter::{
    AddedRoot,
    DependencyType,
    LogEvent,
    LogLevel,
    Reporter,
    RootLog,
    RootMessage,
};
use std::collections::HashMap;

/// `pnpm:root added`: one event per direct dependency once the symlink
/// has been created. pacquet's frozen-lockfile snapshot doesn't
/// preserve npm-alias keys at this layer, so `realName` mirrors `name`
/// except for an alias, whose resolved package name it carries. A
/// `link:` dep carries its resolved target in `linkedFrom`, so the
/// reporter renders `<- <path>` and an embedder's `hideLinkedPkgsDiff`
/// can recognize it. `latest` comes from
/// [`ImporterDependencyGraph::latest_versions`]. The optional `id` field is
/// out of pacquet's reach today and skips from the wire shape rather than
/// serializing as JSON `null`.
pub(super) fn emit_root_added<Reporter: self::Reporter>(
    entry: &ResolvedEntry<'_>,
    graph: &ImporterDependencyGraph<'_>,
    prefix: &str,
) {
    let ResolvedEntry { name, spec, group, name_str, .. } = entry;
    let dependency_type = match group {
        DependencyGroup::Prod => DependencyType::Prod,
        DependencyGroup::Dev => DependencyType::Dev,
        DependencyGroup::Optional => DependencyType::Optional,
        // Filtered upfront. See the comment on the `entries` builder.
        DependencyGroup::Peer => unreachable!("peers are filtered out before this point"),
    };
    let resolved_key = spec.version.resolved_key(name).map(|key| key.without_peer());
    let (version, linked_from) =
        version_and_linked_from(entry, resolved_key.as_ref(), graph.packages);
    let latest = resolved_key
        .as_ref()
        .and_then(|key| graph.latest_versions?.get(key))
        .cloned();
    let real_name = match &spec.version {
        ImporterDepVersion::Alias(alias) => alias.name.to_string(),
        ImporterDepVersion::Regular(_)
        | ImporterDepVersion::Link(_)
        | ImporterDepVersion::File(_) => name.to_string(),
    };
    Reporter::emit(&LogEvent::Root(RootLog {
        level: LogLevel::Debug,
        message: RootMessage::Added {
            prefix: prefix.to_owned(),
            added: AddedRoot {
                name: name_str.clone(),
                real_name,
                version,
                dependency_type: Some(dependency_type),
                id: None,
                latest,
                linked_from,
            },
        },
    }));
}

/// The wire `version` and `linkedFrom` of a direct dep. A `link:` dep has
/// no lockfile version: its target travels in `linkedFrom`, and repeating
/// it as a `link:<path>` version would render the target twice. For
/// `Regular` deps the version is the semver-only formatting on the wire.
/// For an `Alias`, the wire shape is the same as `Regular` (the
/// version-without-peer of the alias's resolved suffix).
fn version_and_linked_from(
    entry: &ResolvedEntry<'_>,
    resolved_key: Option<&PackageKey>,
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
) -> (Option<String>, Option<String>) {
    let ResolvedEntry { spec, target, .. } = entry;
    if matches!(spec.version, ImporterDepVersion::Link(_)) {
        return (None, Some(target.display().to_string()));
    }
    let manifest_version = resolved_key
        .and_then(|key| packages?.get(key))
        .and_then(|metadata| metadata.version.clone());
    (manifest_version.or_else(|| Some(fallback_version(&spec.version))), None)
}
