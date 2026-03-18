---
"@pnpm/installing.deps-restorer": patch
"@pnpm/deps.graph-builder": patch
"pnpm": patch
---

Skip self-dep symlinking and bin linking for warm Global Virtual Store packages. When `safeToSkip` causes the import to be skipped (package already present), post-import linking is also skipped since it was completed during the original import.
