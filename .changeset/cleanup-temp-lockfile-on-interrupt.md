---
"@pnpm/lockfile.fs": patch
"pacquet": patch
"pnpm": patch
---

Interrupting `pnpm install` with Ctrl+C or SIGTERM no longer leaves a temporary lockfile (`.pnpm-lock.yaml.*.tmp`) behind in the project [#1418](https://github.com/pnpm/pnpm/issues/1418).
