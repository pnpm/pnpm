---
"@pnpm/workspace.injected-deps-syncer": patch
"@pnpm/exec.commands": patch
"pnpm": patch
"pacquet": patch
---

With `sharedWorkspaceLockfile: false`, a script listed in `syncInjectedDepsAfterScripts` now updates the injected copies of the package it ran in. Before, those copies kept the files from the last install.
