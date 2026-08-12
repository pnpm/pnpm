---
"@pnpm/workspace.injected-deps-syncer": patch
"pnpm": patch
---

`syncInjectedDepsAfterScripts` no longer fails with `ENOTDIR` when a workspace package replaced a directory with a file of the same name and the injected copy still held that directory's contents.
