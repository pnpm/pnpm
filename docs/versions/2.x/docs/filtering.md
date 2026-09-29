---
id: filtering
title: Filtering
---

Added in: v2.13.0

Filtering allows you to restrict commands to specific subsets of packages.

pnpm supports a rich selector syntax for picking packages by name or by
relation.

Selectors may be specified via the `--filter` flag:

```sh
pnpm <command> --filter <package_selector>
```

> [An article that compares Lerna's filtering to pnpm's](https://medium.com/pnpm/pnpm-vs-lerna-filtering-in-a-multi-package-repository-1f68bc644d6a)

## --filter &lt;package_name\>

Added in: v2.13.0

To select an exact package, just specify its name (`@scope/pkg`) or use a
pattern to select a set of packages (`@scope/*`).

Examples:

```sh
pnpm test --filter "@babel/core"
pnpm test --filter "@babel/*"
pnpm test --filter "*core"
```

## --filter &lt;package_name\>...

Added in: v2.13.0

To select a package and its dependencies (direct and non-direct), suffix the
package name with an ellipsis: `<package_name>...`. For instance, the next
command will run tests of `foo` and all of its dependencies:

```sh
pnpm test --filter foo...
```

You may use a pattern to select a set of root packages:

```sh
pnpm test --filter "@babel/preset-*..."
```

## --filter &lt;package_name\>^...

Added in: v4.4.0

To ONLY select the dependencies of a package (both direct and non-direct),
suffix the name with the aforementioned ellipsis preceded by a chevron. For
instance, the next command will run tests for all of `foo`'s
dependencies:

```sh
pnpm test --filter "foo^..."
```

## --filter ...&lt;package_name\>

Added in: v2.14.0

To select a package and its dependent packages (direct and non-direct), prefix
the package name with an ellipsis: `...<package_name>`. For instance, this will
run the tests of `foo` and all packages dependent on it:

```sh
pnpm test --filter ...foo
```

## --filter "...^&lt;package_name\>"

Added in: v4.4.0

To ONLY select a package's dependents (both direct and non-direct), prefix the
package name with an ellipsis followed by a chevron. For instance, this will
run tests for all packages dependent on `foo`:

```text
pnpm test --filter "...^foo"
```

## --filter ./&lt;directory\>

Added in: v2.15.0

To only select packages under the specified directory, you may specify any
absolute path, typically in POSIX format.

## --filter {&lt;directory\>}

Added in: v4.7.0

Includes all projects that are under the specified directory.

It may be used with the ellipsis and chevron operators to select
dependents/dependencies as well:

```sh
pnpm <cmd> --filter ...{<directory>}
pnpm <cmd> --filter {<directory>}...
pnpm <cmd> --filter ...{<directory>}...
```

It may also be combined with `[<since>]`. For instance, to select all changed
projects inside a directory:

```sh
pnpm <cmd> --filter "{packages}[origin/master]"
pnpm <cmd> --filter "...{packages}[origin/master]"
pnpm <cmd> --filter "{packages}[origin/master]..."
pnpm <cmd> --filter "...{packages}[origin/master]..."
```

Or you may select all packages from a directory with names matching the given
pattern:

```text
pnpm <cmd> --filter "@babel/*{components}"
pnpm <cmd> --filter "@babel/*{components}[origin/master]"
pnpm <cmd> --filter "...@babel/*{components}[origin/master]"
```

## --filter "[&lt;since\>]"

Added in: v4.6.0

Selects all the packages changed since the specified commit/branch. May be
suffixed or prefixed with `...` to include dependencies/dependents.

For example, the next command will run tests in all changed packages since
`master` and on any dependent packages:

```sh
pnpm test --filter "...[origin/master]"
```

## Multiplicity

When packages are filtered, every package is taken that matches at least one of
the selectors. You can use as many filters as you want:

```sh
pnpm test --filter ...foo --filter bar --filter baz...
```

## Excluding

Added in: v5.8.0

Any of the filter selectors may work as exclusion operators when they have a
leading "!". In zsh (and possibly other shells), "!" should be escaped: `\!`.

For instance, this will run a command in all projects except for `foo`:

```sh
pnpm <cmd> --filter=!foo
```

And this will run a command in all projects that are not under the `lib`
directory:

```sh
pnpm <cmd> --filter=!./lib
```

## --test-pattern &lt;glob\>

Added in: v5.14.0

`test-pattern` allows detecting whether the modified files are related to tests.
If they are, the dependent packages of such modified packages are not included.

This option is useful with the "changed since" filter. For instance, the next
command will run tests in all changed packages, and if the changes are in the
source code of the package, tests will run in the dependent packages as well:

```sh
pnpm --filter="...[origin/master]" --test-pattern="test/*" test
```
