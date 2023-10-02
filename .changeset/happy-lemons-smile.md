---
"pnpm": patch
---

Fix memory error in `pnpm why` when the dependencies tree is too big, the command will now prune the tree to just 10 end leafs and now supports `--depth` argument [#7122](https://github.com/pnpm/pnpm/pull/7122).
