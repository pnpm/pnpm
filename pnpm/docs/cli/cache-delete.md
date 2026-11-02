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

Each pattern is a glob segment matched against `<registry>/<pattern>.jsonl`, and only the cache of the configured registry is affected. The cached manifests of single versions that pnpm read from a pnpr registry are deleted with their package. Use `--registry` to pick another one. The deleted files are printed one per line.

```sh
pnpm cache delete '@babel/*'
```

:::warning

Without arguments, `pnpm cache delete` deletes every cached metadata file of the configured registry. The descriptor-scoped metadata under `v12/metadata-private` is not affected.

:::
