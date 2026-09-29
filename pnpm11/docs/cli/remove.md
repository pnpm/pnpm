---
id: remove
title: pnpm remove
---

Aliases: `rm`, `uninstall`, `un`

Removes packages from `node_modules` and from the project's `package.json`.

Since v11.28.0, `pnpm remove` runs the project's own `preuninstall`, `uninstall`, and `postuninstall` scripts. See [Uninstall scripts](../scripts.md#uninstall-scripts).

## Options

### --recursive, -r

When used inside a [workspace](../workspaces.md), removes a dependency (or
dependencies) from every workspace package.

When used not inside a workspace, removes a dependency (or dependencies) from
every package found in subdirectories.

Since v11.28.0, the command fails before any `package.json` is modified if a requested dependency is absent from every selected project. The check respects `--save-prod`, `--save-dev`, and `--save-optional`.

### --global, -g

Remove a global package.

### --save-dev, -D

Only remove the dependency from `devDependencies`.

### --save-optional, -O

Only remove the dependency from `optionalDependencies`.

### --save-prod, -P

Only remove the dependency from `dependencies`.

### --unsafe-perm

Added in: v11.26.0

Accepted for compatibility with `pnpm install`; see [`unsafePerm`](../settings/build.md#unsafeperm).

### Supply-chain policy flags

Added in: v11.26.0

`pnpm remove` accepts the same policy overrides as `pnpm install` and `pnpm add`: `--trust-lockfile`, `--no-trust-lockfile`, [`--trust-policy`](../settings/dependency-resolution.md#trustpolicy), [`--trust-policy-exclude`](../settings/dependency-resolution.md#trustpolicyexclude), and [`--trust-policy-ignore-after`](../settings/dependency-resolution.md#trustpolicyignoreafter).

Removing a package rewrites the lockfile, so `pnpm remove` verifies the whole lockfile against the active policies the way `pnpm install` does, not only the entries of the package being removed. [`--trust-lockfile`](../settings/dependency-resolution.md#trustlockfile) skips that pass entirely.

### --filter &lt;package_selector\>

[Read more about filtering.](../filtering.md)
