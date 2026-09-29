---
id: pnpm-install-pkg
title: "pnpm install <pkg>"
original_id: pnpm-install-pkg
---

Installs a package and any packages that it depends on.
By default, any new package is installed as a prod dependency.

![](/img/demos/pnpm-install-package.svg)

## tl;dr

|Command|Meaning|
|--|--|
|`pnpm i sax`                          |npm package (save to `dependencies`)|
|`pnpm i -D sax`                       |save to `devDependencies`           |
|`pnpm i -O sax`                       |save to `optionalDependencies`      |
|`pnpm i -P sax`                       |save to `dependencies`              |
|`pnpm i sax@next`                     |Specify tag `next`                  |
|`pnpm i sax@3.0.0`                    |Specify version `3.0.0`             |
|`pnpm i sax@">=1 <2.0"`               |Specify version range               |
|`pnpm i user/repo`                    |GitHub                              |
|`pnpm i user/repo#master`             |GitHub                              |
|`pnpm i user/repo#semver:^2.0.0`      |GitHub                              |
|`pnpm i github:user/repo`             |GitHub                              |
|`pnpm i gitlab:user/repo`             |GitHub                              |
|`pnpm i /path/to/repo`                |Absolute path                       |
|`pnpm i ./archive.tgz`                |Tarball                             |
|`pnpm i https://site.com/archive.tgz` |Tarball via HTTP                    |

## Supported package locations

A package can be installed from different locations:

### Install from npm registry

`pnpm install package-name` will install the latest version
of `package-name` from the [npm registry](https://www.npmjs.com/).

You may also install packages by:

* tag: `pnpm install express@nightly`
* version: `pnpm install express@1.0.0`
* version range: `pnpm install express@2 react@">=0.1.0 <0.2.0"`

### Install from local file system

There are two ways to install from the local file system:

1. from a tarball file (`.tar`, `.tar.gz`, or `.tgz`)
2. from a directory

Examples:

```sh
pnpm install ./package.tgz
pnpm install ./some-directory
```

When you install from a directory, a symlink will be created in the
current project's `node_modules`, so it is the same as running
`pnpm link`.

### Install from remote gzipped tarball

The argument must start with "http://" or "https://".

Example:

```sh
pnpm install https://github.com/indexzero/forever/tarball/v0.5.6
```

### Install from Git repository

```sh
npm install <git remote url>
```

Installs the package from the hosted Git provider, cloning it with Git.

You may install from Git by:

* commit: `pnpm install kevva/is-positive#97edff6f525f192a3f83cea1944765f769ae2678`
* branch: `pnpm install kevva/is-positive#master`
* version range: `pnpm install kevva/is-positive#semver:^2.0.0`

## Options

### --save-prod, -P

This will install one or more packages in your `dependencies`.

### --save-dev, -D

Using `--save-dev` or `-D` will install one or more packages in your `devDependencies`.

### --save-optional, -O

Using `--save-optional` or `-O` will install one or more packages in your `optionalDependencies`.

### --save-exact, -E

Saved dependencies will be configured with an exact version rather than using pnpm's default semver range operator.
