---
"@pnpm/reviewing.dependencies-hierarchy": patch
"@pnpm/list": patch
"pnpm": patch
---

Fixed an out-of-memory error in `pnpm list` (and `pnpm why`) on large dependency graphs by replacing the recursive tree builder with a two-phase approach: a BFS dependency graph followed by cached tree materialization. Duplicate subtrees are now deduplicated in the output, shown as "deduped (N deps hidden)" [#10586](https://github.com/pnpm/pnpm/pull/10586).
