---
id: unlink
title: pnpm unlink
---

Unlinks a system-wide package (inverse of [`pnpm link`](./link.md)).

If called without arguments, all linked dependencies will be unlinked inside the
current project.

This is similar to `yarn unlink`, except pnpm re-installs the dependency after
removing the external link.

:::info

If you want to remove a link made with `pnpm link --global <package>`, you should use `pnpm uninstall --global <package>`.
`pnpm unlink` only removes the links in your current directory.

:::

## Options

### --recursive, -r

Unlink in every package found in subdirectories or in every workspace package,
when executed inside a [workspace](../workspaces.md).

### --filter &lt;package_selector\>

[Read more about filtering.](../filtering.md)
