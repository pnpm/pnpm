---
id: update
title: pnpm update
original_id: update
---

Aliases: `up`

`pnpm update` updates packages to their latest version based on the specified range.

## tl;dr

|Command|Meaning|
|--|--|
|`pnpm up` |updates all dependencies. Adheres ranges specified in `package.json` |
|`pnpm up --latest` |updates all dependencies. Ignores ranges specified in `package.json` |
|`pnpm up foo@2` |updates `foo` to the latest v2 |

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
