---
id: unlink
title: pnpm unlink
---

Removes links created by [`pnpm link <dir>`](./link.md) and reinstalls the packages as declared.

If called without arguments, all linked dependencies will be unlinked inside the
current project.

This is similar to `yarn unlink`, except pnpm re-installs the dependency after
removing the external link.

Since v11.28.0, if `pnpm link <dir>` added a `link:` dependency to `package.json`,
`pnpm unlink` removes that entry too, along with the package in `node_modules`
and its lockfile entry. A `link:` dependency that points to another directory is
kept.

## Options

### --recursive, -r

Unlink in every package found in subdirectories or in every workspace package,
when executed inside a [workspace](../workspaces.md).

### --filter &lt;package_selector\>

[Read more about filtering.](../filtering.md)
