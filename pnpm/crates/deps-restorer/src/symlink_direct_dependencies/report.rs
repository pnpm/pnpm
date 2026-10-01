use super::resolve::{ResolvedEntry, fallback_version};
use pnpm_lockfile::{ImporterDepVersion, PackageKey, PackageMetadata};
use pnpm_package_manifest::DependencyGroup;
use pnpm_reporter::{
    AddedRoot, DependencyType, LogEvent, LogLevel, Reporter, RootLog, RootMessage,
};
use std::collections::HashMap;

/// `pnpm:root added`: one event per direct dependency once the symlink
/// has been created. pacquet's frozen-lockfile snapshot doesn't
/// preserve npm-alias keys at this layer, so `realName` mirrors `name`
/// except for an alias, whose resolved package name it carries. A
/// `link:` dep carries its resolved target in `linkedFrom`, as pnpm v11
/// did, so the reporter renders `<- <path>` and an embedder's
/// `hideLinkedPkgsDiff` can recognize it. The optional `id` / `latest`
/// fields are out of pacquet's reach today and skip from the wire shape
/// rather than serializing as JSON `null`.
pub(super) fn emit_root_added<Reporter: self::Reporter>(
    entry: &ResolvedEntry<'_>,
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
    prefix: &str,
) {
    let ResolvedEntry { name, spec, group, name_str, target } = entry;
    let dependency_type = match group {
        DependencyGroup::Prod => DependencyType::Prod,
        DependencyGroup::Dev => DependencyType::Dev,
        DependencyGroup::Optional => DependencyType::Optional,
        // Filtered upfront. See the comment on the `entries` builder.
        DependencyGroup::Peer => unreachable!("peers are filtered out before this point"),
    };
    // A `link:` dep has no lockfile version: its target travels in
    // `linked_from` instead, and repeating it as a `link:<path>` version
    // would render the target twice. For `Regular` deps the version is
    // the semver-only formatting on the wire. For an `Alias`, the wire
    // shape is the same as `Regular` (the version-without-peer of the
    // alias's resolved suffix); the resolved package name surfaces via
    // `real_name`.
    if matches!(spec.version, ImporterDepVersion::Link(_)) {
        emit::<Reporter>(
            prefix,
            AddedRoot {
                name: name_str.clone(),
                real_name: name.to_string(),
                version: None,
                dependency_type: Some(dependency_type),
                id: None,
                latest: None,
                linked_from: Some(target.display().to_string()),
            },
        );
        return;
    }
    let manifest_version = spec.version
        .resolved_key(name)
        .and_then(|key| packages?.get(&key.without_peer()))
        .and_then(|metadata| metadata.version.clone());
    let version = manifest_version.or_else(|| Some(fallback_version(&spec.version)));
    let real_name = match &spec.version {
        ImporterDepVersion::Alias(alias) => alias.name.to_string(),
        ImporterDepVersion::Regular(_)
        | ImporterDepVersion::Link(_)
        | ImporterDepVersion::File(_) => name.to_string(),
    };
    emit::<Reporter>(
        prefix,
        AddedRoot {
            name: name_str.clone(),
            real_name,
            version,
            dependency_type: Some(dependency_type),
            id: None,
            latest: None,
            linked_from: None,
        },
    );
}

fn emit<Reporter: self::Reporter>(prefix: &str, added: AddedRoot) {
    Reporter::emit(&LogEvent::Root(RootLog {
        level: LogLevel::Debug,
        message: RootMessage::Added { prefix: prefix.to_owned(), added },
    }));
}
