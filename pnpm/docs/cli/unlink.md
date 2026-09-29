---
id: unlink
title: pnpm unlink
---

Unlinks a system-wide package (inverse of [`pnpm link`](./link.md)).

If called without arguments, all linked dependencies will be unlinked inside the
current project.

This is similar to `yarn unlink`, except pnpm re-installs the dependency after
removing the external link.

Since v12.7.0, if `pnpm link <dir>` added a `link:` dependency to `package.json`,
`pnpm unlink` removes that entry too, along with the package in `node_modules`
and its lockfile entry. A `link:` dependency that points to another directory is
kept.

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
