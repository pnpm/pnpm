---
"@pnpm/workspace.injected-deps-syncer": patch
"pnpm": patch
---

`syncInjectedDepsAfterScripts` now identifies a file by its device as well as its inode number. An inode number is only unique within one filesystem, so on its own it could match an unrelated file on another device and leave that path stale in the injected copy.
