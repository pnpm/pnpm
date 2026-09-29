---
id: list
title: pnpm list
original_id: list
---

Aliases: `ls`

This command will print to stdout all the versions of packages that are installed, as well as their dependencies, in a tree-structure.

Positional arguments are `name-pattern@version-range` identifiers, which will limit the results to only the packages named.
For example, `pnpm list babel-* eslint-* semver@5`

## Synopsis

```text
pnpm list [-r] [--filter <package selector>] [--depth <number>]
          [<package pattern> ...]

pnpm recursive list [--filter <package selector>] [--depth <number>]
                    [<package pattern> ...]
```

## Options

### --recursive, -r

Perform command on every package in subdirectories
or on every workspace package, when executed inside a workspace.

### --json

Added in: 3.7.0

Show information in JSON format.

### --long

Show extended information.

### --parseable

Show parseable output instead of tree view.

### --global

List packages in the global install prefix instead of in the current project.

### --depth &lt;number>

Max display depth of the dependency tree.

`pnpm ls --depth 0` will list direct dependencies only.
`pnpm ls --depth -1` will list projects only. Useful inside a workspace when used with the `-r` option.

### --prod, --production, --only prod

Display only the dependency tree for packages in `dependencies`.

### --dev, --only dev

Display only the dependency tree for packages in `devDependencies`.

### --filter &lt;package_selector>

[Read more about filtering.](../filtering.md)
