---
id: store
title: pnpm store
---

Managing the package store.

## Commands

### status

Checks for modified packages in the store.

Returns exit code 0 if the content of the package is the same as it was at the
time of unpacking.

### add

Functionally equivalent to [`pnpm add`], except this adds new packages to the
store directly without modifying any projects or files outside of the store.

[`pnpm add`]: ./add.md

### prune

Removes _unreferenced packages_ from the store.

Unreferenced packages are packages that are not used by any projects on the
system. Packages can become unreferenced after most installation operations, for
instance when dependencies are made redundant.

For example, during `pnpm install`, package `foo@1.0.0` is updated to
`foo@1.0.1`. pnpm will keep `foo@1.0.0` in the store, as it does not
automatically remove packages. If package `foo@1.0.0` is not used by any other
project on the system, it becomes unreferenced. Running `pnpm store prune` would
remove `foo@1.0.0` from the store.

Running `pnpm store prune` is not harmful and has no side effects on your
projects. If future installations require removed packages, pnpm will download
them again.

It is best practice to run `pnpm store prune` occasionally to clean up the
store, but not too frequently. Sometimes, unreferenced packages become required
again. This could occur when switching branches and installing older
dependencies, in which case pnpm would need to re-download all removed packages,
briefly slowing down the installation process.

When the [global virtual store] is enabled, `pnpm store prune` also performs mark-and-sweep garbage collection on the global virtual store's `links/` directory. Projects using the store are registered via symlinks in `{storeDir}/v10/projects/`, allowing pnpm to track active usage and safely remove unused packages from the global virtual store.

[global virtual store]: ../settings.md#enableglobalvirtualstore

Please note that this command is prohibited when a [store server] is running.

[store server]: ./server.md

### path

Returns the path to the active store directory.
