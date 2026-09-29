---
"pacquet": patch
---

`pnpm install` without `--frozen-lockfile` is faster on some machines in projects with a `pnpm-workspace.yaml`. Those installs linked with one worker thread per core, half of what a frozen install uses.
