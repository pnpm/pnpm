---
id: pnpm-recursive
title: pnpm recursive
original_id: pnpm-recursive
---

## tl;dr

|Command|Meaning|
|--|--|
|`pnpm install -r` | runs installation for every package in every subfolder |
|`pnpm run build --filter foo-*` |builds all packages with names that start with `foo-` |
|`pnpm update -- login-page...` |updates dependencies in `login-page` and any dependencies of `login-page` that are also in the repository |

## Options

### link-workspace-packages

Added in: v2.14.0

* Default: **true**
* Type: **Boolean**

When `true`, locally available packages are linked to `node_modules` instead of being downloaded from the registry.
This is very convenient in a multi-package repository.

#### Usage

Create a `.npmrc` file in the root of your multi-package repository with the following content:

```
link-workspace-packages = true
```

Create a [pnpm-workspace.yaml](workspace.md) file with the following content:

```yaml
packages:
  - '**'
```

Run `pnpm recursive install`.

### shared-workspace-lockfile

Added in: v2.17.0 (initially named `shared-workspace-shrinkwrap`)

* Default: **true**
* Type: **Boolean**

When `true`, pnpm creates a single `pnpm-lock.yaml` file in the root of the workspace (in the directory that contains the `pnpm-workspace.yaml` file).
A shared lockfile also means that all dependencies of all workspace packages will be in a single `node_modules`.

Advantages of this option:

* every dependency is a singleton
* faster installations in a multi-package repository
* fewer changes in code reviews

**NOTE:** even though all the dependencies will be hard linked into the root `node_modules`, packages will have access only to those dependencies
that are declared in their `package.json`. So pnpm's strictness is preserved.

### -- &lt;package_selector>..., --filter &lt;package_selector>

Added in: v2.13.0

Ability to pass selectors after `--` added in v2.15.0

Filters allow to restrict commands to a subset of packages.
A rich selector syntax is supported for picking packages by name
or by relation.

#### --filter &lt;package_name>

Added in: v2.13.0

To select an exact package, just specify its name (`@babel/core`) or use a pattern
to select a set of packages (`@babel/*`).

Usage examples:

```sh
pnpm recursive install --filter @babel/core
pnpm recursive install --filter @babel/*
# or
pnpm recursive install -- @babel/core
pnpm recursive install -- @babel/*
```

#### --filter &lt;package_name>...

Added in: v2.13.0

To select a package and its dependencies (direct and non-direct), suffix the package name with 3 dots: `<package_name>...`.
For instance, the next command will run installation in all dependencies of `foo` and in `foo`:

```sh
pnpm recursive install --filter foo...
# or
pnpm recursive install -- foo...
```

You may use a pattern to select a set of "root" packages:

```sh
pnpm recursive install --filter @babel/preset-*...
# or
pnpm recursive install -- @babel/preset-*...
```

#### --filter ...&lt;package_name>

Added in: 2.14.0

To select a package and its dependent packages (direct and non-direct), prefix the package name with 3 dots: `...<package_name>`.
For instance, the next command will run installation in all dependents of `foo` and in `foo`:

```sh
pnpm recursive install --filter ...foo
# or
pnpm recursive install -- ...foo
```

When packages in the workspace are filtered, every package is taken that matches at least one of
the selectors. You can use as many filters as you want:

```sh
pnpm recursive install --filter ...foo --filter bar --filter qar...
# or
pnpm recursive install -- ...foo bar qar...
```

#### --filter ./&lt;directory>

Added in: v2.15.0

### workspace-concurrency

Added in: v2.13.0

* Default: **4**
* Type: **Number**

Set the maximum number of concurrency. For unlimited concurrency use `Infinity`.

### bail

Added in: v2.13.0

* Default: **true**
* Type: **Boolean**

If true, stops when a task throws an error.

Usage example. Run tests in every package. Continue if tests fail in one of the packages:

```
pnpm recursive test --no-bail
```

### sort

Added in: v2.14.0

* Default: **true**
* Type: **Boolean**

When `true`, packages are sorted topologically (dependencies before dependents). Pass `--no-sort` to disable.

Usage examples:

```sh
pnpm recursive test --no-sort
```

## pnpm recursive install

Added in: v1.24.0

```sh
pnpm recursive install [arguments]
```

Concurrently runs install in all subdirectories with a `package.json` (excluding node_modules).

Usage examples:

```sh
pnpm recursive install
pnpm recursive install --ignore-scripts
```

## pnpm recursive update

Added in: v1.24.0

```sh
pnpm recursive update [arguments]
```

Concurrently runs update in all subdirectories with a `package.json` (excluding node_modules).

Usage examples:

```sh
pnpm recursive update
pnpm recursive update --depth 100
# update typescript to the latest version in every package
pnpm recursive update typescript@latest
```

## pnpm recursive uninstall

Added in: v2.10.0

```sh
pnpm recursive uninstall [<@scope>/]<pkg>...
```

Uninstall a dependency from each package

Usage examples:

```sh
pnpm recursive uninstall webpack
```

## pnpm recursive dislink

Added in: v1.32.0

An alias of `recursive unlink` from v2.0.0

```sh
pnpm recursive dislink [arguments]
```

Removes links to local packages and reinstalls them from the registry.

Usage examples:

```sh
pnpm recursive dislink
```

## pnpm recursive outdated

Added in: v2.2.0

```sh
pnpm recursive outdated [[<@scope>/]<pkg> ...]
```

Check for outdated packages in every project of the multi-package repo.

Usage examples:

```sh
pnpm recursive outdated
```

## pnpm recursive list

Added in: v2.2.0

```sh
pnpm recursive list [[<@scope>/]<pkg> ...]
```

List packages in each project of the multi-package repo.
Accepts the same arguments and flags as the regular `pnpm list` command.

Usage examples:

```sh
pnpm recursive list
```

## pnpm recursive run

Added in: v2.3.0

```sh
pnpm recursive run <command> [-- <args>...]
```

This runs an arbitrary command from each package's "scripts" object.
If a package doesn't have the command, it is skipped.
If none of the packages have the command, the command fails.

Usage examples:

```sh
pnpm recursive run build
```

In addition to the shell’s pre-existing `PATH`, `pnpm run` adds `node_modules/.bin`
to the `PATH` provided to `scripts`. As of v3.5, when executed inside a workspace,
`<workspace root>/node_modules/.bin` is also added to the `PATH`, so if a tool
is installed in the workspace root, it may be called in any workspace package's `scripts`.

## pnpm recursive test

Added in: v2.3.0

```sh
pnpm recursive test [-- <args>...]
```

This runs each package's "test" script, if one was provided.

Usage examples:

```sh
pnpm recursive test
```

## pnpm recursive rebuild

Added in: v2.4.0

```sh
pnpm recursive rebuild [[<@scope>/<name>]...]
```

This command runs the **pnpm build** command in every package of the multi-package repo.

Usage examples:

```sh
pnpm recursive rebuild
```

## pnpm recursive exec

Added in: v2.9.0

```sh
pnpm recursive exec -- <command> [args...]
```

This command runs a command in each package of the multi-package repo.

The name of the current package is available through the environment variable `PNPM_PACKAGE_NAME` (supported from pnpm v2.22.0).

Usage examples:

```sh
pnpm recursive exec -- rm -rf node_modules
pnpm recursive exec -- pnpm view \$PNPM_PACKAGE_NAME
```
