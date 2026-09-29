---
id: update
title: pnpm update
---

Aliases: `up`, `upgrade`

`pnpm update` updates packages to their latest version based on the specified
range.

When used without arguments, updates all dependencies.

## TL;DR

| Command              | Meaning                                                                  |
|----------------------|--------------------------------------------------------------------------|
|`pnpm up`             | Updates all dependencies, adhering to ranges specified in `package.json` |
|`pnpm up --latest`    | Updates all dependencies to their latest versions                        |
|`pnpm up foo@2`       | Updates `foo` to the latest version on v2                                |
|`pnpm up "@babel/*"` | Updates all dependencies under the `@babel` scope                        |

## Selecting dependencies with patterns

You can use patterns to update specific dependencies.

Update all `babel` packages:

```sh
pnpm update "@babel/*"
```

Update all dependencies, except `webpack`:

```sh
pnpm update "\!webpack"
```

Patterns may also be combined, so the next command will update all `babel` packages, except `core`:

```sh
pnpm update "@babel/*" "\!@babel/core"
```

## Options

### --recursive, -r

Concurrently runs update in all subdirectories with a `package.json` (excluding
node_modules).

Usage examples:

```sh
pnpm --recursive update
# updates all packages up to 100 subdirectories in depth
pnpm --recursive update --depth 100
# update typescript to the latest version in every package
pnpm --recursive update typescript@latest
```

### --latest, -L

Update the dependencies to their latest stable version as determined by their `latest` tags (potentially upgrading the packages across major versions) as long as the version range specified in `package.json` is lower than the `latest` tag (i.e. it will not downgrade prereleases).

### --global, -g

Update global packages.

### --workspace

Tries to link all packages from the workspace. Versions are updated to match the
versions of packages inside the workspace.

If specific packages are updated, the command will fail if any of the updated
dependencies are not found inside the workspace. For instance, the following
command fails if `express` is not a workspace package:

```sh
pnpm up -r --workspace express
```

### --prod, -P

Only update packages in `dependencies` and `optionalDependencies`.

### --dev, -D

Only update packages in `devDependencies`.

### --no-optional

Don't update packages in `optionalDependencies`.

### --interactive, -i

Show outdated dependencies and select which ones to update.

### --no-save

Don't update the ranges in `package.json`.

### --filter &lt;package_selector\>

[Read more about filtering.](../filtering.md)
