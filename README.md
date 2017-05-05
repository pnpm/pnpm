# pnpm

[![Status](https://travis-ci.org/pnpm/pnpm.svg?branch=master)](https://travis-ci.org/pnpm/pnpm "See test builds")
[![Windows build status](https://ci.appveyor.com/api/projects/status/f7437jbcml04x750/branch/master?svg=true)](https://ci.appveyor.com/project/zkochan/pnpm-17nv8/branch/master)
[![Join the chat at https://gitter.im/pnpm/pnpm](https://badges.gitter.im/pnpm/pnpm.svg)](https://gitter.im/pnpm/pnpm?utm_source=badge&utm_medium=badge&utm_campaign=pr-badge&utm_content=badge)

> Fast, disk space efficient npm installs

pnpm is a fast implementation of `npm install`.

[![asciicast](http://i.imgur.com/6GLLHaV.gif)](https://asciinema.org/a/99357)

*Read our [contributing guide](CONTRIBUTING.md) if you're looking to contribute.*

Follow the [pnpm Twitter account](https://twitter.com/pnpmjs) for updates.

## Table of Contents

* [Background](#background)
* [Install](#install)
* [Usage](#usage)
  * [Configuring](#configuring)
* [Benchmark](#benchmark)
* [Frequently Asked Questions](#frequently-asked-questions)
* Recipes
  * [Continuous Integration](docs/recipes/continuous-integration.md)
* Advanced
  * [About the package store](docs/about-the-package-store.md)

## Background

pnpm uses hard links and symlinks to save one version of a module only ever once on a disk.
When using npm or yarn for example, if you have 100 packages using lodash, you will have
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

Use `pnpm` in place of `npm`. It overrides `pnpm i`, `pnpm install` and some other command, the rest will passthru to `npm`.

```
pnpm install lodash
```

For using the programmatic API, see: [API](docs/api.md).

### Configuring

pnpm uses npm's programmatic API to read configs. Hence, you should set configs for pnpm the same way you would for npm.

Furthermore, pnpm uses the same configs that npm uses for doing installations. If you have a private registry and npm is configured
to work with it, pnpm should be able to authorize requests as well, with no additional configuration.

However, pnpm has some unique configs as well:

#### store-path

* Default: **~/.pnpm-store**
* Type: **path**

The location where all the packages are saved on the disk.

#### local-registry

* Default: **~/.pnpm-registry**
* Type: **path**

The location of all the downloaded packages and package meta information.
Can be also used as a [verdaccio](https://github.com/verdaccio/verdaccio) storage.

#### offline

* Default: **false**
* Type: **Boolean**

If true, pnpm will use only the local registry mirror to get packages.
If a package won't be found locally, installation will fail.

#### network-concurrency

* Default: **16**
* Type: **Number**

Controls the maximum number of HTTP requests that can be done simultaneously.

#### child-concurrency

* Default: **5**
* Type: **Number**

Controls the number of child processes run parallely to build node modules.

## Benchmark

pnpm is usually 10 times faster than npm and 30% faster than yarn. See [this](https://github.com/zkochan/node-package-manager-benchmark)
benchmark which compares the three package managers on different types of applications.

```
time npm i babel-preset-es2015 browserify chalk debug minimist mkdirp
    66.15 real        15.60 user         3.54 sys
```

```
time pnpm i babel-preset-es2015 browserify chalk debug minimist mkdirp
    11.04 real         6.85 user         2.85 sys
```

## Prior art

* [Compared to ied](docs/vs-ied.md)
* [Compared to npm](docs/vs-npm.md)

## Frequently Asked Questions

### Why does my `node_modules` folder use disk space if packages are stored in a global store?

pnpm creates [hard links](https://en.wikipedia.org/wiki/Hard_link) from the global store to project's `node_modules` folders.
Hard links point to the same place on the disk where the original files are.
So, for example, if you have `foo` in your project as a dependency and it occupies 1MB of space,
then it will look like it occupies 1MB of space in the project's `node_modules` folder and
the same amount of space in the global store. However, that 1MB is *the same space* on the disk
addressed from two different locations. So in total `foo` occupies 1MB,
not 2MB.

For more on this subject: [Why do hard links seem to take the same space as the originals?](https://unix.stackexchange.com/questions/88423/why-do-hard-links-seem-to-take-the-same-space-as-the-originals)

## License

[MIT](https://github.com/pnpm/pnpm/blob/master/LICENSE)

[contributors]: http://github.com/pnpm/pnpm/contributors
