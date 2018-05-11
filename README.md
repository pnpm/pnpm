![](https://i.imgur.com/qlW1eEG.png)

# pnpm

> Fast, disk space efficient package manager

[![npm version](https://img.shields.io/npm/v/pnpm.svg)](https://www.npmjs.com/package/pnpm)
[![Status](https://travis-ci.org/pnpm/pnpm.svg?branch=master)](https://travis-ci.org/pnpm/pnpm "See test builds")
[![Windows build status](https://ci.appveyor.com/api/projects/status/f7437jbcml04x750/branch/master?svg=true)](https://ci.appveyor.com/project/zkochan/pnpm-17nv8/branch/master)
[![Join the chat at https://gitter.im/pnpm/pnpm](https://badges.gitter.im/pnpm/pnpm.svg)](https://gitter.im/pnpm/pnpm?utm_source=badge&utm_medium=badge&utm_campaign=pr-badge&utm_content=badge)
[![Twitter Follow](https://img.shields.io/twitter/follow/pnpmjs.svg?style=social&label=Follow)](https://twitter.com/pnpmjs)

Features:

* **Fast.** As fast as npm and Yarn.
* **Efficient.** One version of a package is saved only ever once on a disk.
* **Deterministic.** Has a lockfile called `shrinkwrap.yaml`.
* **Strict.** A package can access only dependencies that are specified in its `package.json`.
* **Works everywhere.** Works on Windows, Linux and OS X.
* **Aliases.** Install different versions of the same package or import it using a different name.

Like this project? Let people know with a [tweet](https://bit.ly/tweet-pnpm).

## Table of Contents

* [Background](#background)
* [Install](#install)
* [Usage](#usage)
  * [pnpm CLI](#pnpm-cli)
  * [pnpx CLI](#pnpx-cli)
  * [Configuring](https://pnpm.js.org/docs/en/configuring.html)
  * [Hooks](https://pnpm.js.org/docs/en/hooks.html)
  * [Aliases](https://pnpm.js.org/docs/en/aliases.html)
* [Benchmark](#benchmark)
* [Limitations](https://pnpm.js.org/docs/en/limitations.html)
* [Frequently Asked Questions](https://pnpm.js.org/docs/en/faq.html)
* [Support](#support)
* [Awesome list](https://github.com/pnpm/awesome-pnpm)
* Recipes
  * [Continuous Integration](https://pnpm.js.org/docs/en/continuous-integration.html)
* Advanced
  * [About the package store](https://pnpm.js.org/docs/en/about-package-store.html)
  * [Symlinked `node_modules` structure](https://pnpm.js.org/docs/en/symlinked-node-modules-structure.html)
  * [How peers are resolved](https://pnpm.js.org/docs/en/how-peers-are-resolved.html)
* [Contributing](CONTRIBUTING.md)

## Background

pnpm uses hard links and symlinks to save one version of a module only ever once on a disk.
When using npm or Yarn for example, if you have 100 projects using the same version
of lodash, you will have 100 copies of lodash on disk. With pnpm, lodash will be saved in a
single place on the disk and a hard link will put it into the `node_modules` where it should
be installed.

As a result, you save gigabytes of space on your disk and you have a lot faster installations!
If you'd like more details about the unique `node_modules` structure that pnpm creates and
why it works fine with the Node.js ecosystem, read this small article: [Why should we use pnpm?](https://www.kochan.io/nodejs/why-should-we-use-pnpm.html)

## Install

Using a [standalone script](https://github.com/pnpm/self-installer#readme):

```
curl -L https://unpkg.com/@pnpm/self-installer | node
```

Via npm:

```
npm install -g pnpm
```

Once you first installed pnpm, you can upgrade it using pnpm:

```
pnpm install -g pnpm
```

> Do you wanna use pnpm on CI servers? See: [Continuous Integration](https://pnpm.js.org/docs/en/continuous-integration.html).

## Usage

### pnpm CLI

Just use pnpm in place of npm. For instance, to install run:

```
pnpm install lodash
```

For more advanced usage, read [pnpm CLI](https://pnpm.js.org/docs/en/pnpm-cli.html) on our website.

For using the programmatic API, use pnpm's engine: [supi](https://github.com/pnpm/supi).

### pnpx CLI

npm has a great package runner called [npx](https://medium.com/@maybekatz/introducing-npx-an-npm-package-runner-55f7d4bd282b).
pnpm offers the same tool via the `pnpx` command. The only difference is that `pnpx` uses pnpm for installing packages.

The following command installs a temporary create-react-app and calls it,
without polluting global installs or requiring more than one step!

```
pnpx create-react-app my-cool-new-app
```

## Benchmark

pnpm is faster than npm and Yarn. See [this](https://github.com/zkochan/node-package-manager-benchmark)
benchmark which compares the three package managers on different types of applications.

Here are the benchmarks on a React app:

![](https://cdn.rawgit.com/pnpm/node-package-manager-benchmark/7f0c3e40/results/imgs/react-app.svg)

## Support

- [Stack Overflow](https://stackoverflow.com/questions/tagged/pnpm)
- [Gitter chat](https://gitter.im/pnpm/pnpm)
- [Twitter](https://twitter.com/pnpmjs)

## License

[MIT](https://github.com/pnpm/pnpm/blob/master/LICENSE)
