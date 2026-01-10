---
"@pnpm/plugin-commands-script-runners": minor
---

`pnpm run -r` and `pnpm run --filter` now fail with a non-zero exit code when no packages have the specified script. Previously, this only failed when all packages were selected. Use `--if-present` to suppress this error.
