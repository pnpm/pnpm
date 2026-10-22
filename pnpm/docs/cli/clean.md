---
id: clean
title: pnpm clean
---

Aliases: `purge`

Safely remove `node_modules` contents from all workspace projects. Removes directories without following NTFS junctions on Windows into their targets.

In a workspace, `node_modules` directories are cleaned in the root and every workspace package. Non-pnpm hidden entries (e.g., `.cache`) inside `node_modules` are preserved. A custom [`modulesDir`](../settings/node-modules.md#modulesdir) is cleaned instead of `node_modules`.

A `clean` or `purge` script in `package.json` overrides the built-in command. `pnpm pm clean` and `pnpm pm purge` run the built-in command regardless.

If a custom [`virtualStoreDir`](../settings/node-modules.md#virtualstoredir) or `globalVirtualStoreDir` is configured and it resides inside the project root (but outside `node_modules`), it is also removed.

## Options

### --lockfile, -l

Also remove the `pnpm-lock.yaml` of the workspace root, or of the project outside a workspace.
