---
id: clean
title: pnpm clean
---

Aliases: `purge`

Safely remove `node_modules` contents from all workspace projects. Removes directories without following NTFS junctions on Windows into their targets.

In a workspace, `pnpm clean` removes `node_modules` in the root and every workspace package by default, even when the workspace root has no `package.json`. Passing [`--filter`](../filtering.md), `--filter-prod`, `--workspace-root`, or `--no-recursive` cleans only the selected projects and preserves the shared virtual store (`node_modules/.pnpm`) when the workspace uses a shared lockfile. `pnpm clean` preserves non-pnpm hidden entries (e.g., `.cache`) inside `node_modules` and cleans a custom [`modulesDir`](../settings/node-modules.md#modulesdir) when set.

A `clean` or `purge` script in `package.json` overrides the built-in command. `pnpm pm clean` and `pnpm pm purge` run the built-in command regardless.

If a custom [`virtualStoreDir`](../settings/node-modules.md#virtualstoredir) or `globalVirtualStoreDir` resides inside the project root (but outside `node_modules`), an unfiltered `pnpm clean` removes it too.

## Options

### --lockfile, -l

Also remove the `pnpm-lock.yaml` files of the cleaned projects and, when `pnpm clean` cleans the workspace root, a lockfile inside [`lockfileDir`](../settings/store.md#lockfiledir) if that directory is inside the workspace root.

### --lockfile-dir &lt;dir\>

The directory that holds `pnpm-lock.yaml`. `--lockfile` removes the lockfile there when that directory is inside the workspace or project root.

### --filter &lt;package_selector\>

[Read more about filtering.](../filtering.md)
