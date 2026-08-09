## 1103.2.0

### Minor Changes

- Lockfile verification now honors offline mode by using cached registry metadata instead of reaching the registry. When the required metadata is not available locally, verification reports the same `ERR_PNPM_NO_OFFLINE_META` condition used by offline resolution.

### Patch Changes

- The held-back-update warning printed by `pnpm update` no longer fires when `minimumReleaseAge` is the actual reason a newer version was not picked. The warning's baseline now applies the same maturity cutoff as the pick itself, so it no longer wrongly attributes the hold-back to "your manifests and already installed dependencies" or recommends an override that would defeat the age gate. See pnpm/pnpm#13071.

- Updated dependencies:
  - @pnpm/resolving.registry.pkg-metadata-filter@1100.0.16
