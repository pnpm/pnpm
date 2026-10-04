## 1101.0.9

### Patch Changes

- `pnpm install --frozen-lockfile` now succeeds in a project with no dependencies when `pnpm-lock.yaml` records only the pinned pnpm version. Other commands write such a lockfile when they run before the first install. A lockfile missing the `---` line after that section is accepted too [#16477](https://github.com/pnpm/pnpm/issues/16477).

- Updated dependencies:
  - @pnpm/core-loggers@1101.0.2
  - @pnpm/installing.read-projects-context@1101.0.9
  - @pnpm/lockfile.fs@1100.2.12
  - @pnpm/store.controller@1102.2.3
