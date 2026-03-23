---
"@pnpm/config.deps-installer": patch
"pnpm": patch
---

Fixed inconsistent behavior with `--frozen-lockfile`. When using `--frozen-lockfile`, pnpm will no longer attempt to modify `pnpm-workspace.yaml` during config dependency migration, preventing failures in read-only environments like Docker containers.

Fixes #10829.
