---
"@pnpm/workspace.injected-deps-syncer": patch
"@pnpm/exec.commands": patch
"pacquet": patch
"pnpm": patch
---

`syncInjectedDepsAfterScripts` now removes the bin link of a bin the script dropped. Previously only new bins were linked, so a build step that stopped declaring one left its shim behind, pointing at a command that was no longer there.
