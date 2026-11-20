## 1100.1.30

### Patch Changes

- `pnpm --if-present start`, `pnpm run --if-present start`, and `pnpm restart --if-present` now exit with code 0 when a project has neither a `start` script nor a `server.js` file [#18073](https://github.com/pnpm/pnpm/pull/18073).

- `pnpm version -r` now runs each bumped package's `preversion`, `version`, and `postversion` scripts unless `--ignore-scripts` or `--dry-run` is set. `pnpm version -r --dry-run --json` now prints the planned releases as a JSON array.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.43
  - @pnpm/core-loggers@1101.0.5
  - @pnpm/exec.npm-lifecycle@1100.0.9
  - @pnpm/fetching.directory-fetcher@1100.0.45
  - @pnpm/pkg-manifest.reader@1100.0.25
  - @pnpm/store.controller-types@1101.3.7
  - @pnpm/types@1102.1.4
  - @pnpm/workspace.task-scheduler@1100.0.8
