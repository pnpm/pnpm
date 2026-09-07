---
"@pnpm/config.reader": patch
"pacquet": patch
"pnpm": patch
---

A relative `scriptShell` path in `pnpm-workspace.yaml` is now resolved against the workspace root, so scripts run from a nested workspace package find the shell [#14422](https://github.com/pnpm/pnpm/issues/14422). A bare command name such as `bash` is still looked up on `PATH`.
