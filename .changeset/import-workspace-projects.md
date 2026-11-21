---
"pacquet": patch
---

`pnpm import` now writes each project's lockfile when `sharedWorkspaceLockfile` is disabled and respects `disallowWorkspaceCycles`. It normalizes workspace-relative Yarn patch paths and accepts Yarn Classic entries identified by a URL or file path.
