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
| `pnpm add lodash@npm:awesome-lodash`   | Install `awesome-lodash` as `lodash` |
| `pnpm add crate:serde`                 | Add a [Cargo](../cargo.md) crate   |
| `pnpm add pypi:httpx`                  | Add a [Python](../python.md) package |

## Supported package sources

pnpm supports installing packages from various sources. See the [Supported package sources](../package-sources.md) page for detailed documentation on:

- npm registry
- JSR registry
- Workspace packages
- Local file system (tarballs and directories)
- Remote tarballs
- Git repositories (with semver, subdirectories, and more)
- crates.io and PyPI, through the `crate:` and `pypi:` prefixes

## Package aliases

To install an npm package under a different local name, use an `npm:` alias:

```sh
pnpm add <alias>@npm:<package>
pnpm add <alias>@npm:<package>@<version-or-tag>
```

The package after `npm:` may be scoped and versioned. For example:

```sh
pnpm add babel-core@npm:@babel/core@8.0.6
```

This writes `babel-core` as the dependency name while resolving
`@babel/core@8.0.6` as the target package. The `npm:` part is the npm
package-alias protocol, not a [named registry](../package-sources.md#named-registries);
registry routing still follows the target package's default or scope-specific
npm registry configuration.

See [Aliases](../aliases.md) for more examples.

## Adding a package manager or a runtime

Added in: v12.0.0-rc.6 (pnpm v12 only)

Naming a [package manager](../package-managers.md) — `npm`, `yarn` or `bun` — records which one the project uses instead of installing the npm package that shares the name:

```sh
pnpm add yarn@4
```

writes `"packageManager": "yarn@4.18.0"`, and every other package manager is recorded as a range in [`devEngines.packageManager`](../package_json.md#devenginespackagemanager). Naming a runtime (`node`, `deno`) records it under `engines.runtime`, as the explicit `node@runtime:22` spelling already did — `bun` is both, and is declared as the project's package manager unless you ask for the runtime (`pnpm add bun@runtime:1.3.0`).

Globally, `pnpm add -g yarn` installs the current Yarn line rather than the Classic-only `yarn` package, and `pnpm add -g node@22` installs that Node.js release rather than a wrapper that downloads one.

A specifier that locates a package rather than asking for a released version — `pnpm add yarn@npm:yarn@1.22.22`, `pnpm add yarn@yarnpkg/berry` — installs what it names, as an ordinary dependency.

## Protocol-prefixed selectors

A selector may carry the protocol in front of the name rather than after it:

```sh
pnpm add jsr:@scope/pkg
pnpm add npm:pkg@^1.0.0
pnpm add workspace:pkg@*
```

## Package URLs

Added in: v12.5.0

`pnpm add` accepts [Package URLs](https://github.com/package-url/purl-spec) for npm, Cargo, and Python packages:

```sh
pnpm add pkg:npm/express@4.18.2
pnpm add pkg:cargo/serde@1.0.188
pnpm add pkg:pypi/requests@2.31.0
```

These save dependencies to `package.json`, `Cargo.toml`, and `pyproject.toml`, respectively. Cargo and Python use their ecosystem configuration. `pkg` is a reserved specifier prefix, regardless of case, and cannot be used as a named registry alias.

## Options

### --save-prod, -P, -p

Install the specified packages as regular `dependencies`.

### --save-dev, -D, -d

Install the specified packages as `devDependencies`.

### --save-optional, -O, -o

Install the specified packages as `optionalDependencies`.

### --save-exact, -E, -e

Saved dependencies will be configured with an exact version rather than using
pnpm's default semver range operator.

### --save-peer

Using `--save-peer` will add one or more packages to `peerDependencies` and
install them as dev dependencies.

### --save-types

Added in: v12.6.0

Saves available `@types/*` packages in `devDependencies` alongside registry
dependencies. Packages that declare bundled TypeScript types are skipped.

```sh
pnpm add express --save-types
# saves express to dependencies, @types/express to devDependencies
```

Set `saveTypes: true` in `pnpm-workspace.yaml` to enable this by default.

### --tilde

Added in: v12.6.0

Alias for `--save-prefix=~`. Saved dependencies will be configured with a tilde
range (`~1.2.3`) instead of the default caret range (`^1.2.3`).

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

Install a package globally. See [Global Packages](../global-packages.md) for details.

Each space-separated package is installed into its own isolated directory. To bundle several packages into a single isolated install (so they share dependencies and are removed together), pass them as a comma-separated list, e.g. `pnpm add -g eslint,prettier`.

### --workspace

Only adds the new dependency if it is found in the workspace. The dependency is
saved with the [`workspace:` protocol](../workspaces.md#workspace-protocol-workspace)
and linked from the workspace project that provides it. When no workspace
project provides the package, the command fails instead of falling back to the
registry.


### --allow-build

Added in: v10.4.0

A list of package names that are allowed to run postinstall scripts during installation.

Example:

```
pnpm --allow-build=esbuild add my-bundler
```

This will run `esbuild`'s postinstall script and also add it to the `allowBuilds` field of `pnpm-workspace.yaml`. So, `esbuild` will always be allowed to run its scripts in the future.

Since v12.4.0, prefixing a name with `!` denies the build instead:

```
pnpm add --allow-build=!core-js my-bundler
```

writes `allowBuilds: { core-js: false }`, so the package is never asked about again. Global installs (`pnpm add -g`) record the denial too.

### --filter &lt;package_selector\>

[Read more about filtering.](../filtering.md)

import CpuFlag from '../settings/_cpuFlag.mdx'

<CpuFlag />

import OsFlag from '../settings/_osFlag.mdx'

<OsFlag />

import LibcFlag from '../settings/_libcFlag.mdx'

<LibcFlag />
