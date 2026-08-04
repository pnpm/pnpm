---
"pacquet": patch
---

Global commands (`pnpm add -g`, `pnpm runtime set -g`, ...) now create a missing global bin directory instead of failing with `ERR_PNPM_PNPM_DIR_NOT_WRITABLE`, and the universal `--silent` / `-s` shorthands for `--reporter=silent` (e.g. `pnpm store path --silent`) are supported again.
