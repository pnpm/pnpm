---
id: add
title: "pnpm add <pkg>"
---

Installs a package and any packages that it depends on.
By default, any new package is installed as a production dependency.

## TL;DR

| Command                                | Meaning                            |
|----------------------------------------|------------------------------------|
| `pnpm add sax`                         | Save to `dependencies`             |
| `pnpm add -D sax`                      | Save to `devDependencies`          |
| `pnpm add -O sax`                      | Save to `optionalDependencies`     |
| `pnpm add -g sax `                     | Install package globally           |
| `pnpm add sax@next`                    | Install from the `next` tag        |
| `pnpm add sax@3.0.0`                   | Specify version `3.0.0`            |

## Supported package sources

pnpm supports installing packages from various sources. See the [Supported package sources](../package-sources.md) page for detailed documentation on:

- npm registry
- JSR registry
- Workspace packages
- Local file system (tarballs and directories)
- Remote tarballs
- Git repositories (with semver, subdirectories, and more)

## Options

### --save-prod, -P

Install the specified packages as regular `dependencies`.

### --save-dev, -D

Install the specified packages as `devDependencies`.

### --save-optional, -O

Install the specified packages as `optionalDependencies`.

### --save-exact, -E

Saved dependencies will be configured with an exact version rather than using
pnpm's default semver range operator.

### --save-peer

Using `--save-peer` will add one or more packages to `peerDependencies` and
install them as dev dependencies.

### --save-catalog

Added in: v10.12.1

Save the new dependency to the default [catalog].

### --save-catalog-name &lt;catalog_name\>

Added in: v10.12.1

Save the new dependency to the specified [catalog].

[catalog]: catalogs.md

### --config

Added in: v10.8.0

Save the dependency to [configDependencies](config-dependencies.md).

### --ignore-workspace-root-check

Adding a new dependency to the root workspace package fails, unless the
`--ignore-workspace-root-check` or `-w` flag is used.

For instance, `pnpm add debug -w`.

### --global, -g

Install a package globally.

### --workspace

Only adds the new dependency if it is found in the workspace.


### --allow-build

Added in: v10.4.0

A list of package names that are allowed to run postinstall scripts during installation.

Example:

```
pnpm --allow-build=esbuild add my-bundler
```

This will run `esbuild`'s postinstall script and also add it to the `onlyBuiltDependencies` field of `pnpm-workspace.yaml`. So, `esbuild` will always be allowed to run its scripts in the future.

### --filter &lt;package_selector\>

[Read more about filtering.](../filtering.md)

import CpuFlag from '../settings/_cpuFlag.mdx'

<CpuFlag />

import OsFlag from '../settings/_osFlag.mdx'

<OsFlag />

import LibcFlag from '../settings/_libcFlag.mdx'

<LibcFlag />
