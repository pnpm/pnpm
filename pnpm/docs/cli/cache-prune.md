---
id: cache-prune
title: pnpm cache prune
---

Added in: v12.5.0 (pnpm v12 only)

:::warning

This command is experimental

:::

Deletes the registry metadata cache directories that this version of pnpm can no longer read.

```sh
pnpm cache prune
```

pnpm v12.4.0 changed the metadata cache directory name of every registry, so the first install after upgrading refetched metadata into a new directory and left the old one in place. Nothing reads it, and no other subcommand removes it, because [`cache list`](./cache-list.md), [`cache view`](./cache-view.md) and [`cache delete`](./cache-delete.md) all search under the configured registry's current directory name. `cache prune` goes through all three metadata roots, `v11/metadata`, `v11/metadata-full` and `v11/metadata-full-filtered`, and removes the directories whose names this version cannot read.

Each removed directory is printed as the metadata root followed by the directory name:

```
v11/metadata/registry.npmjs.org
v11/metadata-full/npm.example.com
```

A cache with nothing to reclaim prints nothing and exits successfully. When one directory cannot be read or removed, prune still reclaims the rest, then reports every failure and names the directory it happened on.

## Options

### --dry-run

Lists what prune would delete and removes nothing.

```sh
pnpm cache prune --dry-run
```

The list on stdout is the same as a real prune's, so you can diff the two runs or pipe the list into something else. The notice that nothing was deleted goes to stderr.

:::warning

The cache directory is shared by every pnpm on the machine, and the directory naming change shipped in v11.27.0 as well as v12.4.0. If pnpm v11.26 or earlier runs on the same machine under the same [`cacheDir`](../settings/other.md#cachedir), it still reads and writes the directories that prune removes, and pruning from v12 costs that CLI one metadata refetch. Only the cache is affected, and it refills itself. Use `--dry-run` to see the list before removing anything.

:::

## What prune keeps

Prune removes only the directories it can prove are dead, so a few things that look reclaimable stay where they are.

- The descriptor-scoped caches under `v11/metadata-private`, which every `pnpm cache` subcommand leaves alone.
- Directories whose names are in the current shape, including the ones that pile up from registries on an ephemeral port, such as a local test registry. Removing those needs a retention policy, an age cutoff for example, rather than a rule about the name.
- A 64-character hexadecimal directory name. A registry URL too long to fit a directory name becomes a bare sha256 hash of itself, and a live cache under such a name cannot be told apart from a stale one, so prune keeps it.
