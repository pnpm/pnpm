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
/// except for an alias, whose resolved package name it carries; the
/// optional `id` / `latest` / `linkedFrom` fields are out of pacquet's
/// reach today and skip from the wire shape rather than serializing as
/// JSON `null`.
pub(super) fn emit_root_added<Reporter: self::Reporter>(
    entry: &ResolvedEntry<'_>,
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
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
    // For a `link:` dep, the `version` field is the resolved
    // `link:<path>` payload (re-prepended on the wire) so reporters can
    // render the link target; for `Regular` deps it is the semver-only
    // formatting on the wire. For an `Alias`, the wire shape is the same
    // as `Regular` (the version-without-peer of the alias's resolved
    // suffix); the resolved package name surfaces via `real_name`.
    let manifest_version = spec
        .version
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
                latest: None,
                linked_from: None,
            },
        },
    }));
}
