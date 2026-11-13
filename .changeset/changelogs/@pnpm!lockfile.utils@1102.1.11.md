## 1102.1.11

### Patch Changes

- `pnpm deploy --offline --trust-lockfile` now reuses dependencies pinned in the source lockfile from a populated store when registry metadata is unavailable. This works with per-project lockfiles, `--legacy`, and npm aliases [pnpm/pnpm#17888](https://github.com/pnpm/pnpm/issues/17888).

- Updated dependencies:
  - @pnpm/deps.path@1101.0.9
  - @pnpm/hooks.types@1101.0.8
  - @pnpm/lockfile.types@1100.1.7
  - @pnpm/resolving.resolver-base@1101.3.5
