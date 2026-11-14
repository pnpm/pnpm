---
id: clean
title: pnpm clean
---

Aliases: `purge`

Safely remove `node_modules` contents from all workspace projects. Uses Node.js to remove directories, which correctly handles NTFS junctions on Windows without following them into their targets.

In a workspace, `node_modules` directories are cleaned in the root and every workspace package by default, even when the workspace root has no `package.json`. Passing [`--filter`](../filtering.md), `--filter-prod`, or `--workspace-root` cleans only the selected projects. Non-pnpm hidden entries (e.g., `.cache`) inside `node_modules` are preserved.

If a custom [`virtualStoreDir`](../settings/node-modules.md#virtualstoredir) is configured and it resides inside the project root (but outside `node_modules`), an unfiltered `pnpm clean` removes it too.

## Options

### --lockfile, -l

Also remove the `pnpm-lock.yaml` files of the cleaned projects and, when the workspace root is cleaned, a lockfile inside [`lockfileDir`](../settings/lockfile.md#lockfiledir) if that directory is inside the workspace root.
