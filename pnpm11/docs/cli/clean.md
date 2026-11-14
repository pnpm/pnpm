---
id: clean
title: pnpm clean
---

Aliases: `purge`

Safely remove `node_modules` contents from all workspace projects. Uses Node.js to remove directories, which correctly handles NTFS junctions on Windows without following them into their targets.

In a workspace, `pnpm clean` removes `node_modules` in the root and every workspace package by default, even when the workspace root has no `package.json`. Passing [`--filter`](../filtering.md), `--filter-prod`, or `--workspace-root` cleans only the selected projects. `pnpm clean` preserves non-pnpm hidden entries (e.g., `.cache`) inside `node_modules`.

If a custom [`virtualStoreDir`](../settings/node-modules.md#virtualstoredir) resides inside the project root (but outside `node_modules`), an unfiltered `pnpm clean` removes it too.

## Options

### --lockfile, -l

Also remove the `pnpm-lock.yaml` files of the cleaned projects and, when `pnpm clean` cleans the workspace root, a lockfile inside [`lockfileDir`](../settings/lockfile.md#lockfiledir) if that directory is inside the workspace root.

### --lockfile-dir &lt;dir\>

The directory that holds `pnpm-lock.yaml`. `--lockfile` removes the lockfile there when that directory is inside the workspace or project root.

### --filter &lt;package_selector\>

[Read more about filtering.](../filtering.md)
