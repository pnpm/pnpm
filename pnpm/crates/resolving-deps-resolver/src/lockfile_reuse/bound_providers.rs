pub(crate) type BoundCache =
    rustc_hash::FxHashMap<pnpm_lockfile::PkgNameVerPeer, pnpm_lockfile::BoundProviders>;

pub(crate) fn cached_bound_providers(
    lockfile: &pnpm_lockfile::Lockfile,
    key: &pnpm_lockfile::PkgNameVerPeer,
    bound_cache: &mut BoundCache,
) -> pnpm_lockfile::BoundProviders {
    let providers = pnpm_lockfile::BoundProviders::from_key(key);
    if !providers.is_hashed() {
        return providers;
    }
    bound_cache
        .entry(key.clone())
        .or_insert_with(|| pnpm_lockfile::BoundProviders::of_snapshot(lockfile.into(), key))
        .clone()
}
