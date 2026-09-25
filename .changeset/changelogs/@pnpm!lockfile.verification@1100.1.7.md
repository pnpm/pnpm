## 1100.1.7

### Patch Changes

- `pnpm install --frozen-lockfile` now rejects changed local tarballs, even when the previous archive contents are in the store [pnpm/pnpm#1889](https://github.com/pnpm/pnpm/issues/1889).

- `pnpm install --frozen-lockfile` now succeeds when an optional dependency was unresolvable and skipped by the install that wrote the lockfile. Previously, frozen installs failed with `ERR_PNPM_OUTDATED_LOCKFILE`. The notice states that the dependency could not be resolved and names the requested range [pnpm/pnpm#3960](https://github.com/pnpm/pnpm/issues/3960).

- `pnpm install --frozen-lockfile` now fails when a workspace package's version no longer satisfies the range that a dependent workspace project declares for it. This includes injected workspace dependencies [#7823](https://github.com/pnpm/pnpm/issues/7823).

- `pnpm install` now updates an injected workspace dependency after that package's own dependencies change, when `shared-workspace-lockfile` is `false` [#7209](https://github.com/pnpm/pnpm/issues/7209).

- Updated dependencies:
  - @pnpm/crypto.hash@1100.0.6
  - @pnpm/deps.path@1101.0.4
  - @pnpm/error@1100.2.0
  - @pnpm/installing.context@1101.0.6
  - @pnpm/lockfile.types@1100.1.3
  - @pnpm/lockfile.utils@1102.1.4
  - @pnpm/pkg-manifest.reader@1100.0.20
  - @pnpm/resolving.resolver-base@1101.3.1
  - @pnpm/types@1102.1.1
