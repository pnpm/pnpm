---
"@pnpm/deps.graph-sequencer": patch
"pnpm": patch
"pacquet": patch
---

Topologically sorting workspace projects now runs in linear time, fixing installs and lockfile updates that stalled for seconds on workspaces with thousands of projects forming deep dependency chains [#14149](https://github.com/pnpm/pnpm/issues/14149), [#14151](https://github.com/pnpm/pnpm/issues/14151).
