---
id: why
title: pnpm why
---

Shows all packages that depend on the specified package.

```sh
pnpm why <pkg>...
```

For example:

```sh
pnpm why "@babel/*"
```

A package name or [`--find-by`](#--find-by-finder_name) is required. Without either, the command fails with `ERR_PNPM_MISSING_PACKAGE_NAME`.

The output is a reverse dependency tree: the searched package appears at the root, with its dependents as branches, walking back to workspace roots.

Duplicate subtrees are deduplicated in the output and shown as "deduped".

## Options

### --recursive, -r

Show the dependency tree for the specified package on every package in
subdirectories or on every workspace package when executed inside a workspace.

### --json

Show information in JSON format.

### --long

Show verbose output.

### --parseable

Show parseable output instead of tree view.

### --prod, -P, --production

Only display the dependency tree for packages in `dependencies` and `optionalDependencies`.

### --dev, -D

Only display the dependency tree for packages in `devDependencies`.

### --no-optional

Don't display packages from `optionalDependencies`.

### --optional

Include packages from `optionalDependencies`.

### --depth &lt;number\>

Display only dependencies within a specific depth.

### --exclude-peers

Accepted for compatibility, but has no effect. `pnpm why` always includes peer dependencies.

### --filter &lt;package_selector\>

[Read more about filtering.](../filtering.md)

import FindBy from '../settings/_findBy.mdx'

<FindBy />
