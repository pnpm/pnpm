## 1100.0.9

### Patch Changes

- `pnpm version -r` now runs each bumped package's `preversion`, `version`, and `postversion` scripts unless `--ignore-scripts` or `--dry-run` is set. `pnpm version -r --dry-run --json` now prints the planned releases as a JSON array.

- Updated dependencies:
  - @pnpm/cli.meta@1100.1.5
