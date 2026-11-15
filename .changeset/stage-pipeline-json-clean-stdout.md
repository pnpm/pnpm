---
"pacquet": patch
---

`pnpm stage` with `--json` now keeps `stdout` valid JSON during dry runs, empty workspace runs, and errors. `pnpm pipeline --dry-run --json` and `pnpm -r run --dry-run --json` now write reporter warnings and scope lines to `stderr`.
