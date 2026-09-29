---
id: rebuild
title: pnpm rebuild
original_id: rebuild
---

Aliases: `rb`

Rebuild a package.

## Synopsis

```text
pnpm rebuild [-r [--workspace-concurrency=<number>] [--no-sort]]
     [<pkg>...]
```

## Options

### --recursive, -r

This command runs the **pnpm build** command in every package of the multi-package repo.

### --filter &lt;package_selector>

[Read more about filtering.](../filtering.md)
