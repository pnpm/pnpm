---
id: cache-list
title: pnpm cache list
---

:::warning

This command is experimental

:::

Lists the available packages metadata cache. Supports filtering by glob.

```sh
pnpm cache list [<pattern>...]
```

The command searches the cache of the configured registry, in the metadata directory that the current resolution mode reads. Each match is printed on its own line as a path relative to that directory, `<registry-key>/<name>.jsonl`. Without arguments, every package in it is listed.

```sh
pnpm cache list 'react*'
```
