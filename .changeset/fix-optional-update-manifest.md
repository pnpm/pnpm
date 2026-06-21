---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Fix failed optional dependency updates so they don't rewrite unrelated dependency specs [#11267](https://github.com/pnpm/pnpm/issues/11267).
