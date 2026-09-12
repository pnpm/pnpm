## 1101.0.3

### Patch Changes

- `pnpm sbom` now omits package author fields when the manifest author name is empty or contains only whitespace [pnpm/pnpm#14685](https://github.com/pnpm/pnpm/issues/14685). In a filtered or split workspace run, only a project with no `author` field inherits the workspace root's author.

- `pnpm sbom --sbom-format spdx` now writes `creationInfo.created` with whole seconds, such as `2026-09-08T10:38:21Z`. The timestamp carried fractional seconds, which strict SPDX consumers rejected [#14684](https://github.com/pnpm/pnpm/issues/14684).

- Updated dependencies:
  - @pnpm/lockfile.detect-dep-types@1100.0.23
  - @pnpm/lockfile.utils@1102.1.2
  - @pnpm/lockfile.walker@1100.0.23
  - @pnpm/store.pkg-finder@1100.0.33
