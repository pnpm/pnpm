---
id: pnpm-vs-npm
title: pnpm vs npm
original_id: pnpm-vs-npm
---

## npm's flat tree

npm maintains a [flattened dependency tree](https://github.com/npm/npm/issues/6912) as of version 3.
This leads to less disk space bloat, with a messy `node_modules` directory as a side effect.

On the other hand, pnpm manages `node_modules` as an addressable storage in its [store layout](about-the-package-store.md).
This nets you the benefits of less disk space usage, while keeping your `node_modules` clean.

The good thing about pnpm's proper `node_modules` structure is that it [helps to avoid silly bugs](https://www.kochan.io/nodejs/pnpms-strictness-helps-to-avoid-silly-bugs.html) by making it impossible to use modules
that are not specified in the project's `package.json`.

## Installation

pnpm does not allow installations of packages without saving them to `package.json`.
If no parameters are passed to `pnpm install`, packages are saved as regular dependencies.
Like with npm, `--save-dev` and `--save-optional` can be used to install packages as dev or optional dependencies.

As a consequence of this limitation, projects won't have any extraneous packages when they use pnpm.
That's why pnpm's implementation of the [prune command](https://docs.npmjs.com/cli/prune) does not
have the possibility of prunning specific packages. pnpm's prune always removes all the extraneous packages.

## Directory dependencies

Directory dependencies are the ones that start with the `file:` prefix and point to a directory in the filesystem.
Like npm, pnpm symlinks those dependencies. Unlike npm, pnpm does not perform installation for the file dependencies.
So if you have package `foo` (in `home/src/foo`), that has a dependency `bar@file:../bar`, pnpm won't perform installation in `/home/src/bar`.

If you need to run installations in several packages at the same time (maybe you have a monorepo), you might want to use [pnpmr](https://github.com/pnpm/pnpmr). pnpmr searches for packages and runs `pnpm install` for them in the correct order.
