## 1104.2.10

### Patch Changes

- Resolving against a registry that sends `Cache-Control: no-cache`, such as Azure Artifacts, is fast again when the lockfile already decides the version. pnpm reads the cached metadata for a pinned version, or for a range the lockfile version is proven to win, without a registry request [#17758](https://github.com/pnpm/pnpm/issues/17758).

- `pnpm deploy --offline --trust-lockfile` now reuses dependencies pinned in the source lockfile from a populated store when registry metadata is unavailable. This works with per-project lockfiles, `--legacy`, and npm aliases [pnpm/pnpm#17888](https://github.com/pnpm/pnpm/issues/17888).

- Updated dependencies:
  - @pnpm/crypto.hash@1100.0.9
  - @pnpm/deps.path@1101.0.9
  - @pnpm/fs.graceful-fs@1100.2.6
  - @pnpm/pkg-manifest.utils@1100.4.12
  - @pnpm/resolving.resolver-base@1101.3.5
  - @pnpm/store.cafs@1100.3.10
  - @pnpm/store.index@1101.0.4
