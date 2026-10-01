---
id: remove
title: pnpm remove
---

Aliases: `rm`, `uninstall`, `un`

Removes packages from `node_modules` and from the project's `package.json`.

## Options

### --recursive, -r

When used inside a [workspace](../workspaces.md), removes a dependency (or
dependencies) from every workspace package.

When used not inside a workspace, removes a dependency (or dependencies) from
every package found in subdirectories.

### --global, -g

Remove a global package.

### --save-dev, -D

Only remove the dependency from `devDependencies`.

### --save-optional, -O

Only remove the dependency from `optionalDependencies`.

### --save-prod, -P

Only remove the dependency from `dependencies`.

### Supply-chain policy flags

Added in: v12.3.0

`pnpm remove` accepts the same policy overrides as `pnpm install` and `pnpm add`: `--trust-lockfile`, `--no-trust-lockfile`, [`--trust-policy`](../settings/dependency-resolution.md#trustpolicy), [`--trust-policy-exclude`](../settings/dependency-resolution.md#trustpolicyexclude), and [`--trust-policy-ignore-after`](../settings/dependency-resolution.md#trustpolicyignoreafter).

Removing a package rewrites the lockfile, so `pnpm remove` verifies the whole lockfile against the active policies the way `pnpm install` does, not only the entries of the package being removed. [`--trust-lockfile`](../settings/dependency-resolution.md#trustlockfile) skips that pass entirely.

### --filter &lt;package_selector\>

[Read more about filtering.](../filtering.md)
