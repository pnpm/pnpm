---
"@pnpm/installing.commands": patch
"pnpm": patch
---

With `sharedWorkspaceLockfile: false`, `pnpm install` now runs the `preResolution` hooks and custom resolvers and fetchers of the workspace root's pnpmfile for every project, next to the project's own. A global pnpmfile now runs once per project, and each project's lockfile records the checksum of the root pnpmfile when the project has none of its own. Before, a project's own pnpmfile replaced these root hooks, the global hooks ran twice, and edits to the root pnpmfile could go unnoticed.
