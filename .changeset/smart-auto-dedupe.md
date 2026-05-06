---
"@pnpm/installing.deps-resolver": minor
"@pnpm/installing.deps-installer": minor
"@pnpm/installing.commands": minor
"@pnpm/config.reader": minor
"pnpm": minor
---

Added a new opt-in setting `smartAutoDedupe`. When enabled, after dependencies have been resolved on `pnpm install`, pnpm runs a backtracking pass over the dependency graph: for every parent → child edge, if a higher version of the same package already exists in the graph (sharing the same resolved peer set) and that higher version satisfies the original spec range that requested the child, the edge is rewritten to point at the higher version. Orphaned snapshots are then pruned from the lockfile. The pass is `O(E)` over the parent → child edges in the graph and is gated behind `smart-auto-dedupe = true` (disabled by default). This restores the automatic dedupe behavior that was removed in [#11110](https://github.com/pnpm/pnpm/pull/11110), without reintroducing the non-determinism that motivated that fix — see [#11110 (comment)](https://github.com/pnpm/pnpm/pull/11110#discussion_r2999229543) for the design discussion.
