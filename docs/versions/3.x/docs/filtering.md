---
id: filtering
title: Filtering
original_id: filtering
---

Added in: v2.13.0 (Ability to pass selectors after `--` added in v2.15.0)

Filtering allows to restrict commands to subsets of packages.

pnpm supports a rich selector syntax for picking packages by name
or by relation.

Selectors may be specified via the `--filter` flag:

```text
pnpm <command> --filter <package_selector>
```

Most of the commands, also allow passing selectors after `--`.
Except commands that run scripts (`pnpm run`, `pnpm start`, `pnpm test`, etc).

```text
pnpm <command> -- &lt;package_selectors>...
```

> An article that compares Lerna's filtering to pnpm's: https://medium.com/pnpm/pnpm-vs-lerna-filtering-in-a-multi-package-repository-1f68bc644d6a

## --filter &lt;package_name>

Added in: v2.13.0

To select an exact package, just specify its name (`@babel/core`) or use a pattern
to select a set of packages (`@babel/*`).

Usage examples:

```sh
pnpm install --filter @babel/core
pnpm install --filter @babel/*
# or
pnpm install -- @babel/core
pnpm install -- @babel/*
```

## --filter &lt;package_name>...

Added in: v2.13.0

To select a package and its dependencies (direct and non-direct), suffix the package name with 3 dots: `<package_name>...`.
For instance, the next command will run installation in all dependencies of `foo` and in `foo`:

```sh
pnpm install --filter foo...
# or
pnpm install -- foo...
```

You may use a pattern to select a set of "root" packages:

```sh
pnpm install --filter @babel/preset-*...
# or
pnpm install -- @babel/preset-*...
```

## --filter ...&lt;package_name>

Added in: 2.14.0

To select a package and its dependent packages (direct and non-direct), prefix the package name with 3 dots: `...<package_name>`.
For instance, the next command will run installation in all dependents of `foo` and in `foo`:

```sh
pnpm install --filter ...foo
# or
pnpm install -- ...foo
```

When packages in the workspace are filtered, every package is taken that matches at least one of
the selectors. You can use as many filters as you want:

```sh
pnpm install --filter ...foo --filter bar --filter qar...
# or
pnpm install -- ...foo bar qar...
```

## --filter ./&lt;directory>

Added in: v2.15.0
