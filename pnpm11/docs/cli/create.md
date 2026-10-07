---
id: create
title: "pnpm create"
---

Create a project from a `create-*` or `@foo/create-*` starter kit.

## Examples

```sh
pnpm create react-app my-app
```

## Options

### --allow-build

Added in: v10.2.0

A list of package names that are allowed to run postinstall scripts during installation.

## Security and trust policies

Since v11.0.0, `pnpm create` honors the project-level security and trust policy settings — [`minimumReleaseAge`](../settings/dependency-resolution.md#minimumreleaseage) (and its `Exclude`/`Strict` companions) and [`trustPolicy`](../settings/dependency-resolution.md#trustpolicy) (and its `Exclude`/`IgnoreAfter` companions) — when resolving and fetching the starter kit.
