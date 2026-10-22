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

Only the cache of the configured registry is listed. Each match is printed on its own line as a path relative to the metadata cache directory, `<registry-key>/<name>.jsonl`. Without arguments, every cached package of the registry is listed.

```sh
pnpm cache list 'react*'
```
