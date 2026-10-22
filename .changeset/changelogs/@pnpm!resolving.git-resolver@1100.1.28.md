## 1100.1.28

### Patch Changes

- `pnpm install --frozen-lockfile` now fails with `ERR_PNPM_OUTDATED_LOCKFILE` when a local directory dependency, such as an injected workspace package, changes a dependency to a different tarball URL or git commit [#17129](https://github.com/pnpm/pnpm/issues/17129).

- Updated dependencies:
  - @pnpm/network.fetch@1100.1.23
  - @pnpm/resolving.resolver-base@1101.3.3
