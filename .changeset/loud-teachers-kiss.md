---
"@pnpm/workspace.pkgs-graph": patch
"pnpm": patch
---

When sorting packages in a workspace, take into account workspace dependencies specified as `peerDependencies` [#7813](https://github.com/pnpm/pnpm/issues/7813).
