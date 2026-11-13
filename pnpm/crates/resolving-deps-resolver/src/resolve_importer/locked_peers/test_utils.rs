pub(super) mod legacy_bindings;

use super::{HashMap, HashSet, PkgName, all_peer_versions, hashed_suffix, retained_peer_versions};

pub(in crate::resolve_importer) fn importer_locked_peer_versions(
    wanted_lockfile: Option<&pnpm_lockfile::Lockfile>,
    importer_id: &str,
    is_retained: impl Fn(&PkgName) -> bool,
) -> HashMap<String, HashSet<String>> {
    let Some(lockfile) = wanted_lockfile else {
        return HashMap::default();
    };
    let Some(importer) = lockfile.importers.get(importer_id) else {
        return all_locked_peer_versions(lockfile);
    };
    retained_peer_versions(
        lockfile,
        importer,
        is_retained,
        &hashed_suffix::ProviderContexts::default(),
    )
}

/// Every peer version the lockfile pins anywhere, for an importer it does not
/// list.
pub(in crate::resolve_importer) fn all_locked_peer_versions(
    lockfile: &pnpm_lockfile::Lockfile,
) -> HashMap<String, HashSet<String>> {
    all_peer_versions(lockfile, &hashed_suffix::ProviderContexts::default())
}

impl Default for hashed_suffix::ProviderContexts {
    fn default() -> Self {
        Self::Legacy(std::cell::RefCell::default())
    }
}
