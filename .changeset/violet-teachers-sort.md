---
"@pnpm/deps.graph-sequencer": patch
"pnpm": patch
"pacquet": patch
---

Topologically sorting workspace projects now runs in linear time. On a workspace with thousands of projects forming deep dependency chains, the sort was quadratic and stalled every install for seconds in the `pnpm:scope` and `pnpm:package-manifest` phases [#14149](https://github.com/pnpm/pnpm/issues/14149), which also made lockfile updates on huge monorepos slower than they should be [#14151](https://github.com/pnpm/pnpm/issues/14151).
