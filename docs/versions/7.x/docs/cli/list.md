---
id: list
title: pnpm list
---

Aliases: `ls`

This command will output all the versions of packages that are installed, as
well as their dependencies, in a tree-structure.

Positional arguments are `name-pattern@version-range` identifiers, which will
limit the results to only the packages named. For example,
`pnpm list "babel-*" "eslint-*" semver@5`.

## Options

### --recursive, -r

Perform command on every package in subdirectories or on every workspace
package, when executed inside a workspace.

### --json

Log output in JSON format.

### --long

Show extended information.

### --parseable

Outputs package directories in a parseable format instead of their tree view.

### --global, -g

List packages in the global install directory instead of in the current project.

### --depth &lt;number\>

Max display depth of the dependency tree.

`pnpm ls --depth 0` will list direct dependencies only.
`pnpm ls --depth -1` will list projects only. Useful inside a workspace when
used with the `-r` option.

### --prod, -P

Display only the dependency graph for packages in `dependencies` and
`optionalDependencies`.

### --dev, -D

Display only the dependency graph for packages in `devDependencies`.

### --no-optional

Don't display packages from `optionalDependencies`.

### --only-projects

Added in: v7.22.0

Display only dependencies that are also projects within the workspace.

### --filter &lt;package_selector\>

[Read more about filtering.](../filtering.md)
