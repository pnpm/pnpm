---
id: filtering
title: Filtering
---

Filtering allows you to restrict commands to specific subsets of packages.

pnpm supports a rich selector syntax for picking packages by name or by
relation.

Selectors may be specified via the `--filter` (or `-F`) flag:

```sh
pnpm --filter <package_selector> <command>
```

## Matching

### --filter &lt;package_name>

To select an exact package, just specify its name (`@scope/pkg`) or use a
pattern to select a set of packages (`@scope/*`).

Examples:

```sh
pnpm --filter "@babel/core" test
pnpm --filter "@babel/*" test
pnpm --filter "*core" test
```

Specifying the scope of the package is optional, so `--filter=core` will pick `@babel/core` if `core` is not found.
However, if the workspace has multiple packages with the same name (for instance, `@babel/core` and `@types/core`),
then filtering without scope will pick nothing.

### --filter &lt;package_name>...

To select a package and its dependencies (direct and non-direct), suffix the
package name with an ellipsis: `<package_name>...`. For instance, the next
command will run tests of `foo` and all of its dependencies:

```sh
pnpm --filter foo... test
```

You may use a pattern to select a set of root packages:

```sh
pnpm --filter "@babel/preset-*..." test
```

### --filter &lt;package_name>^...

To ONLY select the dependencies of a package (both direct and non-direct),
suffix the name with the aforementioned ellipsis preceded by a chevron. For
instance, the next command will run tests for all of `foo`'s
dependencies:

```sh
pnpm --filter "foo^..." test
```

### --filter ...&lt;package_name>

To select a package and its dependent packages (direct and non-direct), prefix
the package name with an ellipsis: `...<package_name>`. For instance, this will
run the tests of `foo` and all packages dependent on it:

```sh
pnpm --filter ...foo test
```

### --filter "...^&lt;package_name>"

To ONLY select a package's dependents (both direct and non-direct), prefix the
package name with an ellipsis followed by a chevron. For instance, this will
run tests for all packages dependent on `foo`:

```text
pnpm --filter "...^foo" test
```

### --filter ./&lt;glob>, --filter \{&lt;glob>\}

A glob pattern relative to the current working directory matching projects.

```sh
pnpm --filter "./packages/**" <cmd>
```

Includes all projects that are under the specified directory.

It may be used with the ellipsis and chevron operators to select
dependents/dependencies as well:

```sh
pnpm --filter ...{<directory>} <cmd>
pnpm --filter {<directory>}... <cmd>
pnpm --filter ...{<directory>}... <cmd>
```

It may also be combined with `[<since>]`. For instance, to select all changed
projects inside a directory:

```sh
pnpm --filter "{packages/**}[origin/master]" <cmd>
pnpm --filter "...{packages/**}[origin/master]" <cmd>
pnpm --filter "{packages/**}[origin/master]..." <cmd>
pnpm --filter "...{packages/**}[origin/master]..." <cmd>
```

Or you may select all packages from a directory with names matching the given
pattern:

```text
pnpm --filter "@babel/*{components/**}" <cmd>
pnpm --filter "@babel/*{components/**}[origin/master]" <cmd>
pnpm --filter "...@babel/*{components/**}[origin/master]" <cmd>
```

### --filter "[&lt;since>]"

Selects all the packages changed since the specified commit/branch. May be
suffixed or prefixed with `...` to include dependencies/dependents.

For example, the next command will run tests in all changed packages since
`master` and on any dependent packages:

```sh
pnpm --filter "...[origin/master]" test
```

## Excluding

Any of the filter selectors may work as exclusion operators when they have a
leading "!". In zsh (and possibly other shells), "!" should be escaped: `\!`.

For instance, this will run a command in all projects except for `foo`:

```sh
pnpm --filter=!foo <cmd>
```

And this will run a command in all projects that are not under the `lib`
directory:

```sh
pnpm --filter=!./lib <cmd>
```

## Multiplicity

When packages are filtered, every package is taken that matches at least one of
the selectors. You can use as many filters as you want:

```sh
pnpm --filter ...foo --filter bar --filter baz... test
```

## --filter-prod &lt;filtering_pattern>

Acts the same a `--filter` but omits `devDependencies` when selecting dependency projects
from the workspace.

## --test-pattern &lt;glob>

`test-pattern` allows detecting whether the modified files are related to tests.
If they are, the dependent packages of such modified packages are not included.

This option is useful with the "changed since" filter. For instance, the next
command will run tests in all changed packages, and if the changes are in the
source code of the package, tests will run in the dependent packages as well:

```sh
pnpm --filter="...[origin/master]" --test-pattern="test/*" test
```

## --changed-files-ignore-pattern &ltglob>

Allows to ignore changed files by glob patterns when filtering for changed projects since the specified commit/branch.

Usage example:

```sh
pnpm --filter="...[origin/master]" --changed-files-ignore-pattern="**/README.md" run build
```
