---
"@pnpm/config.reader": patch
"pnpm": patch
---

Resolve package-manager bootstrap dependencies from trusted user or CLI registries and reject package-manager env-lockfile records that do not use registry package paths with integrity-only resolutions before auto-switch execution.
