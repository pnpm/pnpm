---
id: unlink
title: pnpm unlink
---

Unlinks a system-wide package (inverse of [`pnpm link`](./link.md)).

If called without arguments, all linked dependencies will be unlinked.

This is similar to `yarn unlink`, except pnpm re-installs the dependency after
removing the external link.

## Options

### --recursive, -r

Unlink in every package found in subdirectories or in every workspace package,
when executed inside a [workspace](../workspaces.md).

### --filter &lt;package_selector\>

[Read more about filtering.](../filtering.md)
