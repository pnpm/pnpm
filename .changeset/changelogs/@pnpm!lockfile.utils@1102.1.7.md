## 1102.1.7

### Patch Changes

- Registry revisions are no longer supported. pnpm now fails with `ERR_PNPM_REGISTRY_REVISIONS_UNSUPPORTED` on registry metadata that selects a revision, on a `+rN` version specifier, and on a lockfile entry with a `revision` field. `pnpm update --patches` is removed.

- Updated dependencies:
  - @pnpm/config.normalize-registries@1101.0.4
  - @pnpm/deps.path@1101.0.7
  - @pnpm/hooks.types@1101.0.6
  - @pnpm/lockfile.types@1100.1.5
  - @pnpm/resolving.resolver-base@1101.3.3
  - @pnpm/resolving.tarball-url@1101.1.5
  - @pnpm/types@1102.1.2
