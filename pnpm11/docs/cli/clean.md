---
id: clean
title: pnpm clean
---

Aliases: `purge`

Safely remove `node_modules` contents from all workspace projects. Uses Node.js to remove directories, which correctly handles NTFS junctions on Windows without following them into their targets.

In a workspace, `node_modules` directories are cleaned in the root and every workspace package. Non-pnpm hidden entries (e.g., `.cache`) inside `node_modules` are preserved.

If a custom [`virtualStoreDir`](../settings/node-modules.md#virtualstoredir) is configured and it resides inside the project root (but outside `node_modules`), it is also removed.

## Options

### --lockfile, -l

Also remove `pnpm-lock.yaml` files.
