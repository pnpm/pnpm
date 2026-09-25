---
"pacquet": patch
---

`pnpm install` now runs `node --version` once per run. A workspace whose projects keep their own lockfiles (`sharedWorkspaceLockfile: false`) previously ran the probe once or twice for every project, and on macOS the concurrent launches waited on each other, so a project could wait several seconds before its linking started.
