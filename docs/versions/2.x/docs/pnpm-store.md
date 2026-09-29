---
id: pnpm-store
title: pnpm store
original_id: pnpm-store
---

Commands related to the [pnpm package store](about-the-package-store.md).

## pnpm store status

```sh
pnpm store status
```

Checks for modified packages in the store.

Returns exit code 0 if the content of the package is the same as it was at the time of unpacking.

## pnpm store add

Added in: v2.12.0

```sh
pnpm store add [<@scope>/]<pkg>...
```

Adds new packages to the pnpm store directly.
Does not modify any projects or files outside the store.

Usage examples:

```sh
pnpm store add express@4 typescript@2
```

## pnpm store usages

Added in: v2.21.0

```sh
pnpm store usages [<@scope>/]<pkg>...
```

Lists all pnpm projects on the current filesystem that depend on the specified packages in the store.

Usage examples:

```sh
pnpm store usages flatmap-stream
pnpm store usages is-odd@3.0.0 is-even@2.0.0
pnpm store usages @babel/core ansi-regex
```

> Note that this command might be slow for very large stores.
> We are working on improving performance.

## pnpm store prune

```sh
pnpm store prune
```

Removes unreferenced (extraneous, orphan) packages from the store.

Pruning the store is not harmful, but might slow down future installations.

Please read [the FAQ](faq.md#what-does-pnpm-store-prune-do-is-it-harmful) for more information on unreferenced packages and `pnpm store prune` best practices.

> This command is prohibited when a [store server](pnpm-server.md) is running.
