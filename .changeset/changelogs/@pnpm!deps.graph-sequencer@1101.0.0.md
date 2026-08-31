## 1101.0.0

### Major Changes

- Workspace install, rebuild, pack, publish, stage, and lifecycle work now starts as soon as its dependencies finish instead of waiting for an unrelated topological group.

### Patch Changes

- Topologically sorting workspace projects now runs in linear time, fixing installs and lockfile updates that stalled for seconds on workspaces with thousands of projects forming deep dependency chains [#14149](https://github.com/pnpm/pnpm/issues/14149), [#14151](https://github.com/pnpm/pnpm/issues/14151).
