use super::{
    HashMap, HashSet, PkgName, all_locked_peer_versions, hashed_suffix, retained_peer_versions,
};

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
        &mut hashed_suffix::ProviderContexts::default(),
    )
}
