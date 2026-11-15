## 1100.1.35

### Patch Changes

- Version-selector `overrides` entries such as `"ms@^2.0.0": "2.1.1"` now apply to dependencies declared with the `catalog:` protocol when the catalog entry's range matches the selector [#17922](https://github.com/pnpm/pnpm/issues/17922).

- Updated dependencies:
  - @pnpm/config.reader@1102.3.9
  - @pnpm/deps.path@1101.0.9
  - @pnpm/hooks.read-package-hook@1100.3.12
  - @pnpm/installing.context@1101.0.16
  - @pnpm/lockfile.fs@1100.2.18
  - @pnpm/lockfile.settings-checker@1100.2.18
  - @pnpm/lockfile.utils@1102.1.11
  - @pnpm/lockfile.verification@1100.1.17
  - @pnpm/resolving.local-resolver@1101.2.9
  - @pnpm/resolving.resolver-base@1101.3.5
  - @pnpm/workspace.projects-reader@1101.1.7
  - @pnpm/workspace.state@1100.0.55
