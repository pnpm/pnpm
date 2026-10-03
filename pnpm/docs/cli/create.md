---
id: create
title: "pnpm create"
---

Create a project from a `create-*` or `@foo/create-*` starter kit.

## Examples

```
pnpm create react-app my-app
```

## Workspace starter kits

Inside a workspace, `pnpm create <name>` runs a workspace project named
`create-<name>` when one exists. For a scoped name such as `@example/app`, it
looks for `@example/create-app`. The starter's `bin` runs in the current
directory with the remaining arguments forwarded to it.

Install the starter's dependencies first with `pnpm install`. `pnpm create`
does not install dependencies for a workspace starter.

To use a published starter even when a workspace project matches, specify a
version or tag, such as `pnpm create app@latest`, or pass `--ignore-workspace`.

## Options

### --allow-build

Added in: v10.2.0

A list of package names that are allowed to run postinstall scripts during installation.

## Security and trust policies

Since v11.0.0, `pnpm create` honors the project-level security and trust policy settings — [`minimumReleaseAge`](../settings/dependency-resolution.md#minimumreleaseage) (and its `Exclude`/`Strict` companions) and [`trustPolicy`](../settings/dependency-resolution.md#trustpolicy) (and its `Exclude`/`IgnoreAfter` companions) — when resolving and fetching the starter kit.
