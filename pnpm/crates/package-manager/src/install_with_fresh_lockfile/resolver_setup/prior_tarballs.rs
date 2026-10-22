use pnpm_lockfile::{Lockfile, LockfileResolution};
use pnpm_resolving_tarball_resolver::PriorTarballEntry;
use pnpm_store_dir::store_index_key;
use std::collections::HashMap;

/// Map every remote tarball the prior lockfile recorded (with an
/// integrity) to its `<integrity>\t<pkg_id>` store-index key, keyed
/// exactly as `snapshot_cache_key` / the install pass address the row,
/// so the [`TarballResolver`](pnpm_resolving_tarball_resolver::TarballResolver) can reuse a warm store entry instead of
/// re-downloading on re-resolution. Git-hosted tarballs are skipped
/// (they key by `gitHostedStoreIndexKey`, not the integrity) and just
/// re-fetch. Empty on a first install.
///
/// Keyed by `pkg_id` — the bare specifier — rather than
/// `resolution.tarball`, because that is what the resolver looks a
/// dependency up by. The two differ once an immutable response has
/// redirected: the lockfile then records the post-redirect URL while the
/// id stays the specifier the manifest asked for.
///
/// A lockfile whose integrities name another hash than `integrity_target`
/// contributes nothing.
pub(super) fn prior_tarball_entries(
    wanted_lockfile: Option<&Lockfile>,
    integrity_target: pnpm_lockfile::IntegrityTarget,
) -> HashMap<String, PriorTarballEntry> {
    wanted_lockfile
        .filter(|lockfile| lockfile.integrity_target() == integrity_target)
        .and_then(|lockfile| lockfile.packages.as_ref())
        .map(|packages| {
            packages
                .iter()
                .filter_map(|(key, metadata)| match &metadata.resolution {
                    LockfileResolution::Tarball(tarball) if tarball.git_hosted != Some(true) => {
                        let integrity = tarball.integrity.clone()?;
                        let pkg_id = key.pkg_id();
                        let store_index_key = store_index_key(&integrity.to_string(), &pkg_id);
                        Some((
                            pkg_id,
                            PriorTarballEntry {
                                integrity,
                                store_index_key,
                                tarball_url: tarball.tarball.clone(),
                            },
                        ))
                    }
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}
