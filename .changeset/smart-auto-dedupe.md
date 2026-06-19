---
"@pnpm/installing.deps-resolver": minor
"@pnpm/installing.deps-installer": minor
"@pnpm/installing.commands": minor
"@pnpm/config.reader": minor
"pnpm": minor
---

Added a new opt-in setting `smartAutoDedupe` (disabled by default). Enable it by adding `smartAutoDedupe: true` to `pnpm-workspace.yaml` (or by passing `--smart-auto-dedupe` on the command line). When enabled, after dependencies have been resolved on `pnpm install`, pnpm runs a backtracking pass over the dependency graph: for every parent → child edge, if a higher version of the same package already exists in the graph and that higher version satisfies the original spec range that requested the child, the edge is rewritten to point at the higher version. Orphaned snapshots are then pruned from the lockfile.

The pass is intentionally conservative. An edge is only rewritten when the source and the candidate are both registry-hosted tarballs that share the same resolved peer set and the same patch hash. Edges to git, workspace, directory, or local-tarball dependencies — and edges whose spec is not a plain semver range — are left untouched, so patched, git, workspace, and local entries are never eligible for a rewrite. The pass never downgrades and never introduces a version that is not already present in the graph.

This restores the automatic dedupe behavior that was removed in [#11110](https://github.com/pnpm/pnpm/pull/11110), without reintroducing the non-determinism that motivated that fix — see [#11110 (comment)](https://github.com/pnpm/pnpm/pull/11110#discussion_r2999229543) for the design discussion.
