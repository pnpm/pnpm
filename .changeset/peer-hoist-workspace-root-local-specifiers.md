---
"@pnpm/installing.deps-resolver": patch
"pacquet": patch
"pnpm": patch
---

`resolvePeersFromWorkspaceRoot` now judges the workspace root's dependencies by whether another project can resolve them to the same package. A root dependency on a workspace package (`workspace:^1.2.3`) satisfies another project's missing peer, and a root dependency declared with a relative `link:` or `file:` path no longer does — that path is taken relative to the project resolving it, so reusing it elsewhere pointed at a different directory. An absolute local path is unaffected [#13373](https://github.com/pnpm/pnpm/issues/13373).
