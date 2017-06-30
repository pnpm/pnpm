# pnpm

[![npm version](https://img.shields.io/npm/v/pnpm.svg)](https://www.npmjs.com/package/pnpm)
[![Status](https://travis-ci.org/pnpm/pnpm.svg?branch=master)](https://travis-ci.org/pnpm/pnpm "See test builds")
[![Windows build status](https://ci.appveyor.com/api/projects/status/f7437jbcml04x750/branch/master?svg=true)](https://ci.appveyor.com/project/zkochan/pnpm-17nv8/branch/master)
[![Join the chat at https://gitter.im/pnpm/pnpm](https://badges.gitter.im/pnpm/pnpm.svg)](https://gitter.im/pnpm/pnpm?utm_source=badge&utm_medium=badge&utm_campaign=pr-badge&utm_content=badge)
[![Twitter Follow](https://img.shields.io/twitter/follow/pnpmjs.svg?style=social&label=Follow)](https://twitter.com/pnpmjs)

> Fast, disk space efficient package manager

pnpm is:

1. **Fast.** Faster than npm and Yarn. This is not a fair battle as we organize
dependencies in a completely different way. Nevertheless, dependencies installed with
pnpm are Node.js-compatible!
1. **Efficient.** One version of a package is saved only ever once on a disk.
1. **Simple and human-deterministic.** Flattening is hard. A package manager can be
deterministic but no human can easily predict how will a flattened dependency tree look
like. Pnpm not only has a lockfile, the files in its `node_modules` folder are organized
always the same way, the way they are described in `package.json` files.

## Table of Contents

* [Background](#background)
* [Install](#install)
* [Usage](#usage)
  * [Configuring](#configuring)
* [Benchmark](#benchmark)
* [Limitations](#limitations)
* [Frequently Asked Questions](#frequently-asked-questions)
* [Support](#support)
* [Awesome list](https://github.com/pnpm/awesome-pnpm)
* Recipes
  * [Continuous Integration](docs/recipes/continuous-integration.md)
* Advanced
  * [About the package store](docs/about-the-package-store.md)
  * [Symlinked `node_modules` structure](docs/symlinked-node-modules-structure.md)
  * [How peers are resolved](docs/how-peers-are-resolved.md)
* [Contributing](CONTRIBUTING.md)

## Background

pnpm uses hard links and symlinks to save one version of a module only ever once on a disk.
When using npm or Yarn for example, if you have 100 packages using lodash, you will have
100 copies of lodash on disk. With pnpm, lodash will be saved in a single place on the disk
and a hard link will put it into the `node_modules` where it should be installed.

As a result, you save gigabytes of space on your disk and you have a lot faster installations!
If you'd like more details about the unique `node_modules` structure that pnpm creates and
why it works fine with the Node.js ecosystem, read this small article: [Why should we use pnpm?](https://www.kochan.io/nodejs/why-should-we-use-pnpm.html)

## Install

Install it via npm.

```
npm install -g pnpm
```

> Do you wanna use pnpm on CI servers? See: [Continuous Integration](docs/recipes/continuous-integration.md).

## Usage

```
pnpm install lodash
```

Use `pnpm` in place of `npm`. It overrides `install`, `update`, `uninstall`, `link`, `prune` and `install-test`.
The rest of the commands pass through to `npm`.

For using the programmatic API, use pnpm's engine: [supi](https://github.com/pnpm/supi).

### Configuring

pnpm uses npm's programmatic API to read configs. Hence, you should set configs for pnpm the same way you would for npm.

Furthermore, pnpm uses the same configs that npm uses for doing installations. If you have a private registry and npm is configured
to work with it, pnpm should be able to authorize requests as well, with no additional configuration.

However, pnpm has some unique configs as well:

#### store

* Default: **~/.pnpm-store**
* Type: **path**

The location where all the packages are saved on the disk.

#### offline

* Default: **false**
* Type: **Boolean**

If true, pnpm will use only packages already available in the store.
If a package won't be found locally, the installation will fail.

#### network-concurrency

* Default: **16**
* Type: **Number**

Controls the maximum number of HTTP requests that can be done simultaneously.

#### child-concurrency

* Default: **5**
* Type: **Number**

Controls the number of child processes run parallelly to build node modules.

#### lock

* Default: **true**
* Type: **Boolean**

Dangerous! If false, the store is not locked. It means that several installations using the same
store can run simultaneously.

Can be passed in via a CLI option. `--no-lock` to set it to false. E.g.: `pnpm install --no-lock`.

> If you experience issues similar to the ones described in [#594](https://github.com/pnpm/pnpm/issues/594), use this option to disable locking.
> In the meanwhile, we'll try to find a solution that will make locking work for everyone.

#### independent-leaves

* Default: **false**
* Type: **Boolean**

If true, symlinks leaf dependencies directly from the global store. Leaf dependencies are
packages that have no dependencies of their own. Setting this config to `true` might break some packages
that rely on location but gives an average of **8% installation speed improvement**.

## Benchmark

pnpm is faster than npm and Yarn. See [this](https://github.com/zkochan/node-package-manager-benchmark)
benchmark which compares the three package managers on different types of applications.

```
time npm i babel-preset-es2015 browserify chalk debug minimist mkdirp
    66.15 real        15.60 user         3.54 sys
```

```
time pnpm i babel-preset-es2015 browserify chalk debug minimist mkdirp
    11.04 real         6.85 user         2.85 sys
```

## Limitations

1. [npm-shrinkwrap.json](https://docs.npmjs.com/cli/shrinkwrap) is ignored. Unlike pnpm, npm can install the
same `name@version` multiple times and with different sets of dependencies.
npm's shrinkwrap file is designed to reflect the `node_modules` layout created
by npm. pnpm cannot create a similar layout, so it cannot respect
`npm-shrinkwrap.json`.
2. You can't publish npm modules with `bundleDependencies` managed by pnpm.
3. Binstubs (files in `node_modules/.bin`) are always shell files not
symlinks to JS files. The shell files are created to help pluggable CLI apps
in finding their plugins in the unusual `node_modules` structure. This is very
rarely an issue and if you expect the file to be a js file, just reference the
original file instead, as described in [#736](https://github.com/pnpm/pnpm/issues/736).
4. Node.js doesn't work with the [--preserve-symlinks](https://nodejs.org/api/cli.html#cli_preserve_symlinks) flag when executed in a project that uses pnpm.

Got an idea for workarounds for these issues? [Share them.](https://github.com/pnpm/pnpm/issues/new)

## Other Node.js package managers

* `npm`. The oldest and most widely used. See [pnpm vs npm](docs/pnpm-vs-npm.md).
* `ied`. Built on a very similar premise as pnpm. pnpm takes huge inspiration from it.
* `Yarn`. The first Node.js package manager that invented lockfiles and offline installations.

## Frequently Asked Questions

### Why does my `node_modules` folder use disk space if packages are stored in a global store?

pnpm creates [hard links](https://en.wikipedia.org/wiki/Hard_link) from the global store to project's `node_modules` folders.
Hard links point to the same place on the disk where the original files are.
So, for example, if you have `foo` in your project as a dependency and it occupies 1MB of space,
then it will look like it occupies 1MB of space in the project's `node_modules` folder and
the same amount of space in the global store. However, that 1MB is *the same space* on the disk
addressed from two different locations. So in total `foo` occupies 1MB,
not 2MB.

For more on this subject:

* [Why do hard links seem to take the same space as the originals?](https://unix.stackexchange.com/questions/88423/why-do-hard-links-seem-to-take-the-same-space-as-the-originals)
* [A thread from the pnpm chat room](https://gist.github.com/zkochan/106cfef49f8476b753a9cbbf9c65aff1)
* [An issue in the pnpm repo](https://github.com/pnpm/pnpm/issues/794)

### Does it work on Windows? It is harder to create symlinks on Windows

Using symlinks on Windows is problematic indeed. That is why pnpm uses junctions instead of symlinks on Windows OS.

### Does it work on Windows? Nested `node_modules` approach is basically incompatible with Windows

Early versions of npm had issues because of nesting all `node_modules` (see [Node's nested node_modules approach is basically incompatible with Windows](https://github.com/nodejs/node-v0.x-archive/issues/6960)). However, pnpm does not create deep folders, it stores all packages flatly and uses symlinks to create the dependency tree structure.

### What about circular symlinks?

Although pnpm uses symlinks to put dependencies into `node_modules` folders, circular symlinks are avoided because parent packages are placed into the same `node_modules` folder in which their dependencies are. So `foo`'s dependencies are not in `foo/node_modules` but `foo` is in `node_modules/foo`, together with its own dependencies.

### Why have hard links at all? Why not symlink directly to the global store?

One package can have different sets of dependencies on one machine.

In project **A** `foo@1.0.0` can have dependency resolved to `bar@1.0.0` but in project **B** the same dependency of `foo` might
resolve to `bar@1.1.0`. So pnpm hard links `foo@1.0.0` to every project where it is used, in order to create different sets
of dependencies for it.

Direct symlinking to the global store would work with Node's `--preserve-symlinks` flag. But `--preserve-symlinks` comes
with a bunch of different issues, so we decided to stick with hard links.
For more details about why this decision was made, see: https://github.com/nodejs/node-eps/issues/46.

## Support

- [Stack Overflow](https://stackoverflow.com/questions/tagged/pnpm)
- [Gitter chat](https://gitter.im/pnpm/pnpm)
- [Twitter](https://twitter.com/pnpmjs)

## License

[MIT](https://github.com/pnpm/pnpm/blob/master/LICENSE)
