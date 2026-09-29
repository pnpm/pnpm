---
id: update
title: pnpm update
original_id: update
---

Aliases: `up`

`pnpm update` updates packages to their latest version based on the specified range.

When used without arguments, updates all dependencies.

You can use patterns to update all dependencies that match it.

## Synopsis

```text
pnpm update [-r] [--filter <package selector>]
            [<package pattern> ...]

pnpm recursive update [--filter <package selector>]
                      [<package pattern> ...]
```

## tl;dr

|Command|Meaning|
|--|--|
|`pnpm up` |updates all dependencies. Adheres ranges specified in `package.json` |
|`pnpm up --latest` |updates all dependencies. Ignores ranges specified in `package.json` |
|`pnpm up foo@2` |updates `foo` to the latest v2 |
|`pnpm up @babel/*` |updates all dependencies with the `@babel` scope |

## Options

### --recursive, -r

Concurrently runs update in all subdirectories with a `package.json` (excluding node_modules).

Usage examples:

```sh
pnpm --recursive update
pnpm --recursive update --depth 100
# update typescript to the latest version in every package
pnpm --recursive update typescript@latest
```

### --latest, -L

Added in: v3.2.0

Ignores the version range specified in `package.json`. Instead, the version specified by the `latest` tag will be used (potentially upgrading the packages across major versions).

### --filter &lt;package_selector>

[Read more about filtering.](../filtering.md)

### --global

Update global packages.

### --workspace

Added in: v4.4.0

```text
pnpm [-r] update --workspace [<pkg>...]
```

Tries to link all packages from the workspace. Versions are updated to match the
versions of packages inside the workspace.

If specific packages are updated, the command will fail if any of the updated
dependencies is not found inside the workspace. For instance, the following
command fails if `express` is not a workspace package:
`pnpm up -r --workspace express`.

### --prod, -P

Update packages only in `dependencies` and `optionalDependencies`.

### --dev, -D

Update packages only in `devDependencies`.

### --no-optional

Don't update packages in `optionalDependencies`.

### --interactive, -i

Added in: v4.8.0

Show outdated dependencies and select which ones to update.
