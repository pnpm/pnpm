---
id: store
title: pnpm store
---

Managing the [package store](../about-package-store.md).

## Commands

### status

Checks for modified packages in the store.

Returns exit code 0 if the content of the package is the same as it was at the
time of unpacking.

### add

Added in: v2.12.0

Functionally equivalent to [`pnpm add`], except this adds new packages to the
store directly without modifying any projects or files outside of the store.

[`pnpm add`]: ./add.md

### prune

Removes orphan packages from the store.

Pruning the store will save disk space, however may slow down future
installations involving pruned packages. Ultimately, it is a safe operation,
however not recommended if you have orphaned packages from a package you intend
to reinstall.

Please read [the FAQ] for more information on unreferenced packages and best
practices.

Please note that this is prohibited when a [store server] is running.

[the FAQ]: faq.md#what-does-pnpm-store-prune-do-is-it-harmful
[store server]: ./server.md
