---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
"pacquet": patch
---

With `autoInstallPeers`, an optional peer is no longer bound to a version outside its range that only another dependency's subtree brings in. Before, adding an unrelated dependency could flip that binding and rewrite the peer suffixes of other projects in the lockfile [#18162](https://github.com/pnpm/pnpm/issues/18162).
