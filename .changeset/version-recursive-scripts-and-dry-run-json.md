---
"@pnpm/releasing.commands": patch
"pacquet": patch
"pnpm": patch
---

`pnpm version -r` now runs each bumped package's `preversion`, `version`, and `postversion` scripts unless `--ignore-scripts` or `--dry-run` is set. `pnpm version -r --dry-run --json` now prints the planned releases as a JSON array.
