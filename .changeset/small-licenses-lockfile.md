---
"@pnpm/deps.compliance.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm licenses list` reports no licenses for workspace projects not recorded in the lockfile. It errors when `pnpm-lock.yaml` is missing. Empty results now use the requested text or JSON format [pnpm/tasks#174](https://github.com/pnpm/tasks/issues/174).
