---
id: filtering
title: Filtering
original_id: filtering
---

Added in: v2.13.0

Filtering allows to restrict commands to subsets of packages.

pnpm supports a rich selector syntax for picking packages by name
or by relation.

Selectors may be specified via the `--filter` flag:

```text
pnpm <command> --filter <package_selector>
```

> An article that compares Lerna's filtering to pnpm's: https://medium.com/pnpm/pnpm-vs-lerna-filtering-in-a-multi-package-repository-1f68bc644d6a

## --filter &lt;package_name>

Added in: v2.13.0

To select an exact package, just specify its name (`@babel/core`) or use a pattern
to select a set of packages (`@babel/*`).

Usage examples:

```text
pnpm test --filter @babel/core
pnpm test --filter @babel/*
pnpm test --filter *core
```

## --filter &lt;package_name>...

Added in: v2.13.0

To select a package and its dependencies (direct and non-direct), suffix the package name with 3 dots: `<package_name>...`.
For instance, the next command will run installation in all dependencies of `foo` and in `foo`:

```text
pnpm install --filter foo...
```

You may use a pattern to select a set of "root" packages:

```text
pnpm install --filter @babel/preset-*...
```

## --filter &lt;package_name>^...

Added in: v4.4.0

Selects dependencies of a package (both direct and non-direct). For instance:

```text
pnpm install --filter foo^...
```

## --filter ...&lt;package_name>

Added in: v2.14.0

To select a package and its dependent packages (direct and non-direct), prefix the package name with 3 dots: `...<package_name>`.
For instance, the next command will run installation in all dependents of `foo` and in `foo`:

```text
pnpm install --filter ...foo
```

When packages in the workspace are filtered, every package is taken that matches at least one of
the selectors. You can use as many filters as you want:

```text
pnpm install --filter ...foo --filter bar --filter qar...
```

## --filter ...^&lt;package_name>

Added in: v4.4.0

Selects dependent of a package (both direct and non-direct). For instance:

```text
pnpm install --filter ...^foo
```

## --filter ./&lt;directory>

Added in: v2.15.0

## --filter {&lt;directory>}

Added in: v4.7.0

Includes all projects that are under the specified directory.

It may be used with "..." to select dependents/dependencies as well:

```text
pnpm <cmd> --filter ...{<directory>}
pnpm <cmd> --filter {<directory>}...
pnpm <cmd> --filter ...{<directory>}...
```

It may be combined with `[<since>]`. For instance, to select all changed projects
inside a directory:

```text
pnpm <cmd> --filter {packages}[origin/master]
pnpm <cmd> --filter ...{packages}[origin/master]
pnpm <cmd> --filter {packages}[origin/master]...
pnpm <cmd> --filter ...{packages}[origin/master]...
```

Or you may select all packages from a directory with names matching the given pattern:

```text
pnpm <cmd> --filter @babel/*{components}
pnpm <cmd> --filter @babel/*{components}[origin/master]
pnpm <cmd> --filter ...@babel/*{components}[origin/master]
```

## --filter [&lt;since>]

Added in: v4.6.0

Selects all the packages changed since the specified commit/branch. May be
suffixed or prefixed with `...` to include dependencies/dependents.

For example, the next command will run tests in all changed packages since
`master` and on any dependent packages:

```text
pnpm test --filter ...[origin/master]
```
