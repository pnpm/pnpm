---
"@pnpm/installing.commands": patch
"pnpm": patch
---

With `sharedWorkspaceLockfile: false` and a global pnpmfile, `pnpm install` now runs the global pnpmfile once per project and records the workspace root pnpmfile's checksum in each project's lockfile. Before, the global hooks ran twice, and edits to the root pnpmfile did not mark the lockfiles outdated.
