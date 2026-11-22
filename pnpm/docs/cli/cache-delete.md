---
id: cache-delete
title: pnpm cache delete
---

:::warning

This command is experimental

:::

Deletes metadata cache for the specified packages. Supports patterns.

```sh
pnpm cache delete [<pattern>...]
```

Each pattern matches cached metadata across all registries by default. The cached manifests of single versions that pnpm read from a pnpr registry are deleted with their package. Use `--registry` to restrict deletion to one registry. The deleted files are printed one per line.

```sh
pnpm cache delete '@babel/*'
```

:::warning

Without arguments, `pnpm cache delete` deletes every cached metadata file across all registries unless you pass `--registry`. The descriptor-scoped metadata under `v12/metadata-private` is not affected.

:::
