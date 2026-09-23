---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

`pnpm update` no longer warns "Skip adding ... to the default catalog" for a dependency that already uses `catalog:`. pnpm updates that catalog entry, so the warning was wrong [#13715](https://github.com/pnpm/pnpm/issues/13715).
