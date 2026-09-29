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

## Supported package locations

### Install from npm registry

`pnpm add package-name` will install the latest version of `package-name` from
the [npm registry](https://www.npmjs.com/) by default.

If executed in a workspace, the command will first try to check whether other
projects in the workspace use the specified package. If so, the already used version range
will be installed.

You may also install packages by:

* tag: `pnpm add express@nightly`
* version: `pnpm add express@1.0.0`
* version range: `pnpm add express@2 react@">=0.1.0 <0.2.0"`

[the corresponding guide]: #install-from-remote-tarball

### Install from the workspace

Note that when adding dependencies and working within a [workspace], packages
will be installed from the configured sources, depending on whether or not
[`link-workspace-packages`] is set, and use of the
[`workspace: range protocol`].

[workspace]: ../workspaces.md
[`link-workspace-packages`]: ../workspaces.md#link-workspace-packages
[`workspace: range protocol`]: ../workspaces.md#workspace-ranges-workspace

### Install from local file system

There are two ways to install from the local file system:

1. from a tarball file (`.tar`, `.tar.gz`, or `.tgz`)
2. from a directory

Examples:

```sh
pnpm add ./package.tar.gz
pnpm add ./some-directory
```

When you install from a directory, a symlink will be created in the current
project's `node_modules`, so it is the same as running `pnpm link`.

### Install from remote tarball

The argument must be a fetchable URL starting with "http://" or "https://".

Example:

```sh
pnpm add https://github.com/indexzero/forever/tarball/v0.5.6
```

### Install from Git repository

```sh
pnpm add <git remote url>
```

Installs the package from the hosted Git provider, cloning it with Git.
You can use a protocol for certain Git providers. For example,
`pnpm add github:user/repo`

You may install from Git by:

* latest commit from master: `pnpm add kevva/is-positive`
* commit: `pnpm add kevva/is-positive#97edff6f525f192a3f83cea1944765f769ae2678`
* branch: `pnpm add kevva/is-positive#master`
* version range: `pnpm add kevva/is-positive#semver:^2.0.0`

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

Added in: v3.2.0

Using `--save-peer` will add one or more packages to `peerDependencies` and
install them as dev dependencies.

### --ignore-workspace-root-check, -W

Added in: v3.6.0

Adding a new dependency to the root workspace package fails, unless the
`--ignore-workspace-root-check` or `-W` flag is used.

For instance, `pnpm add debug -W`.

### --global, -g

Install a package globally.

### --workspace

Added in: v4.4.0

Only adds the new dependency if it is found in the workspace.

### --filter &lt;package_selector\>

[Read more about filtering.](../filtering.md)
