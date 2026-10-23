---
id: create
title: "pnpm create"
---

Create a project from a `create-*` or `@foo/create-*` starter kit.

```sh
pnpm create <name> [<args>...]
```

The name is mapped to a starter kit package:

| Name | Package |
|------|---------|
| `foo` | `create-foo` |
| `create-foo` | `create-foo` |
| `@scope` | `@scope/create` |
| `@scope/foo` | `@scope/create-foo` |

pnpm runs the starter kit with [`pnx`](./pnx.md) and passes the remaining arguments to it.

## Examples

```sh
pnpm create vite my-app
```

## Options

### --allow-build

A list of package names that are allowed to run postinstall scripts during installation.

### --shell-mode, -c

Run the command inside of a shell. Uses `/bin/sh` on UNIX and `cmd.exe` on Windows.

import CpuFlag from '../settings/_cpuFlag.mdx'

<CpuFlag />

import OsFlag from '../settings/_osFlag.mdx'

<OsFlag />

import LibcFlag from '../settings/_libcFlag.mdx'

<LibcFlag />

## Security and trust policies

`pnpm create` honors the project-level security and trust policy settings when resolving and fetching the starter kit. These include [`minimumReleaseAge`](../settings/dependency-resolution.md#minimumreleaseage), `minimumReleaseAgeExclude`, `minimumReleaseAgeStrict`, [`trustPolicy`](../settings/dependency-resolution.md#trustpolicy), `trustPolicyExclude`, and `trustPolicyIgnoreAfter`.
