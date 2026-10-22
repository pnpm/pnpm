---
"@pnpm/workspace.injected-deps-syncer": patch
"@pnpm/exec.commands": patch
"pnpm": patch
---

A script listed in `syncInjectedDepsAfterScripts` now updates the injected copies of a package that publishes from `publishConfig.directory`. Before, those copies kept the files from the last install.
