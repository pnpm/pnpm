---
id: unlink
title: pnpm unlink
---

Aliases: `dislink`

Removes links created by [`pnpm link <dir>`](./link.md) and reinstalls the packages as declared.

```sh
pnpm unlink [<pkg>...]
```

`pnpm link` records each link as a `link:` entry in the
[`overrides`](../settings/dependency-resolution.md#overrides) of
`pnpm-workspace.yaml`. `pnpm unlink` removes those overrides, all of them or
only the named ones, and then reinstalls. If no overrides are configured, it
prints `Nothing to unlink` and stops.

This is similar to `yarn unlink`, except pnpm re-installs the dependency after
removing the external link.

Since v12.7.0, if `pnpm link <dir>` added a `link:` dependency to `package.json`,
`pnpm unlink` removes that entry too, along with the package in `node_modules`
and its lockfile entry. A `link:` dependency that points to another directory is
kept.

## Options

### --recursive, -r

Unlink in every package found in subdirectories or in every workspace package,
when executed inside a [workspace](../workspaces.md).

### --ignore-pnpmfile

Don't load the hooks in `.pnpmfile.cjs`, including the pnpmfiles of config dependencies.

### --filter &lt;package_selector\>

[Read more about filtering.](../filtering.md)
