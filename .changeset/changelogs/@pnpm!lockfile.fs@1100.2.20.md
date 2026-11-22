## 1100.2.20

### Patch Changes

- `pnpm import` respects the symlink policy for the destination lockfile and skips Yarn patches outside the workspace.

- Updated `js-yaml` to v5. `pnpm-lock.yaml` is formatted the same as before.

- pnpm can read lockfiles with large YAML merge mappings or ordered maps without the excessive parsing time seen in some cases [#18246](https://github.com/pnpm/pnpm/issues/18246).

- `pnpm-lock.yaml` now quotes strings that look like numbers, such as `+.5`, so they are read back as strings.

- Updated dependencies:
  - @pnpm/deps.path@1101.0.11
  - @pnpm/error@1100.2.3
  - @pnpm/fs.graceful-fs@1100.2.7
  - @pnpm/lockfile.merger@1100.0.31
  - @pnpm/lockfile.types@1100.1.9
  - @pnpm/lockfile.utils@1102.1.13
  - @pnpm/patching.config@1100.1.14
  - @pnpm/types@1102.1.5
