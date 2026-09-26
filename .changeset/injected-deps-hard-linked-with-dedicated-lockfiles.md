---
"@pnpm/installing.commands": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"@pnpm/workspace.injected-deps-syncer": patch
"pnpm": patch
"pacquet": patch
---

With `sharedWorkspaceLockfile: false`, an injected workspace package that has lifecycle scripts is now hard linked into the projects that depend on it. Before, pnpm left a plain copy, so later edits to the package did not reach those projects [#9828](https://github.com/pnpm/pnpm/issues/9828).
