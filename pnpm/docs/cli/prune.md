---
id: prune
title: pnpm prune
---

Removes unnecessary packages.

## Options

### --prod

Remove the packages specified in `devDependencies`.

### --no-optional

Remove the packages specified in `optionalDependencies`.

### --trust-lockfile

Added in: v12.15.0

Skip verifying the lockfile against the supply-chain policies. `pnpm prune` also honors [`trustLockfile`](../settings/dependency-resolution.md#trustlockfile) from the configuration. `--no-trust-lockfile` verifies the lockfile even when the configuration trusts it.

:::warning

The prune command does not support recursive execution on a monorepo currently. To only install production-dependencies in a monorepo `node_modules` folders can be deleted and then re-installed with `pnpm install --prod`.

:::
