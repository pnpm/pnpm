## 1100.2.18

### Patch Changes

- `pnpm deploy --offline --trust-lockfile` now reuses dependencies pinned in the source lockfile from a populated store when registry metadata is unavailable. This works with per-project lockfiles, `--legacy`, and npm aliases [pnpm/pnpm#17888](https://github.com/pnpm/pnpm/issues/17888).

- pnpm now reads the boot document of `pnpm-lock.yaml` when it is written in lockfile format 13. pnpm then switches to the pnpm version such a project pins.

- Updated dependencies:
  - @pnpm/deps.path@1101.0.9
  - @pnpm/fs.graceful-fs@1100.2.6
  - @pnpm/lockfile.merger@1100.0.29
  - @pnpm/lockfile.types@1100.1.7
  - @pnpm/lockfile.utils@1102.1.11
  - @pnpm/patching.config@1100.1.12
