![](https://i.imgur.com/qlW1eEG.png)

# pnpm

> Fast, disk space efficient package manager

[![npm version](https://img.shields.io/npm/v/pnpm.svg)](https://www.npmjs.com/package/pnpm)
[![Status](https://travis-ci.org/pnpm/pnpm.svg?branch=master)](https://travis-ci.org/pnpm/pnpm "See test builds")
[![Windows build status](https://ci.appveyor.com/api/projects/status/f7437jbcml04x750/branch/master?svg=true)](https://ci.appveyor.com/project/zkochan/pnpm-17nv8/branch/master)
[![Join the chat at https://gitter.im/pnpm/pnpm](https://badges.gitter.im/pnpm/pnpm.svg)](https://gitter.im/pnpm/pnpm?utm_source=badge&utm_medium=badge&utm_campaign=pr-badge&utm_content=badge)
[![OpenCollective](https://opencollective.com/pnpm/backers/badge.svg)](#backers)
[![OpenCollective](https://opencollective.com/pnpm/sponsors/badge.svg)](#sponsors)
[![Twitter Follow](https://img.shields.io/twitter/follow/pnpmjs.svg?style=social&label=Follow)](https://twitter.com/pnpmjs)

Features:

* **Fast.** As fast as npm and Yarn.
* **Efficient.** One version of a package is saved only ever once on a disk.
* **Great for multi-package repositories (a.k.a. monorepos).** See the [recursive](https://pnpm.js.org/en/cli/recursive) commands.
* **Strict.** A package can access only dependencies that are specified in its `package.json`.
* **Deterministic.** Has a lockfile called `pnpm-lock.yaml`.
* **Works everywhere.** Works on Windows, Linux and OS X.
* **Aliases.** Install different versions of the same package or import it using a different name.

Like this project? Let people know with a [tweet](https://bit.ly/tweet-pnpm).

## Table of Contents

* [Background](#background)
* [Install](#install)
* [Usage](#usage)
* [Benchmark](#benchmark)
* [Support](#support)
* [Contributors](#contributors)

## Background

pnpm uses hard links and symlinks to save one version of a module only ever once on a disk.
When using npm or Yarn for example, if you have 100 projects using the same version
of lodash, you will have 100 copies of lodash on disk. With pnpm, lodash will be saved in a
single place on the disk and a hard link will put it into the `node_modules` where it should
be installed.

As a result, you save gigabytes of space on your disk and you have a lot faster installations!
If you'd like more details about the unique `node_modules` structure that pnpm creates and
why it works fine with the Node.js ecosystem, read this small article: [Flat node_modules is not the only way](https://medium.com/pnpm/flat-node-modules-is-not-the-only-way-d2e40f7296a3).

## Install

Using a [standalone script](https://github.com/pnpm/self-installer#readme):

```
curl -L https://unpkg.com/@pnpm/self-installer | node
```

Via npm:

```
npm add -g pnpm
```

Once you first installed pnpm, you can upgrade it using pnpm:

```
pnpm add -g pnpm
```

> Do you wanna use pnpm on CI servers? See: [Continuous Integration](https://pnpm.js.org/en/continuous-integration).

## Usage

### pnpm CLI

Just use pnpm in place of npm. For instance, to install run:

```
pnpm install
```

For more advanced usage, read [pnpm CLI](https://pnpm.js.org/en/pnpm-cli) on our website.

For using the programmatic API, use pnpm's engine: [supi](packages/supi).

### pnpx CLI

npm has a great package runner called [npx](https://medium.com/@maybekatz/introducing-npx-an-npm-package-runner-55f7d4bd282b).
pnpm offers the same tool via the `pnpx` command. The only difference is that `pnpx` uses pnpm for installing packages.

The following command installs a temporary create-react-app and calls it,
without polluting global installs or requiring more than one step!

```
pnpx create-react-app my-cool-new-app
```

## Benchmark

pnpm is as fast as npm and Yarn. See all benchmarks [here](https://github.com/pnpm/benchmarks-of-javascript-package-managers).

Benchmarks on a React app:

![](https://cdn.rawgit.com/pnpm/benchmarks-of-javascript-package-managers/b14c3e8/results/imgs/react-app.svg)

## Support

- [Frequently Asked Questions](https://pnpm.js.org/en/faq)
- [Stack Overflow](https://stackoverflow.com/questions/tagged/pnpm)
- [Gitter chat](https://gitter.im/pnpm/pnpm)
- [Twitter](https://twitter.com/pnpmjs)
- [Awesome list](https://github.com/pnpm/awesome-pnpm)

## Contributors

This project exists thanks to all the people who contribute. [[Contribute](CONTRIBUTING.md)].
<a href="../../graphs/contributors"><img src="https://opencollective.com/pnpm/contributors.svg?width=890&button=false" /></a>

### Backers

Thank you to all our backers! 🙏 [[Become a backer](https://opencollective.com/pnpm#backer)]

<a href="https://opencollective.com/pnpm#backers" target="_blank"><img src="https://opencollective.com/pnpm/backers.svg?width=890"></a>

### Sponsors

Support this project by becoming a sponsor. Your logo will show up here with a link to your website. [[Become a sponsor](https://opencollective.com/pnpm#sponsor)]

<a href="https://opencollective.com/pnpm/sponsor/0/website" target="_blank"><img src="https://opencollective.com/pnpm/sponsor/0/avatar.svg"></a>
<a href="https://opencollective.com/pnpm/sponsor/1/website" target="_blank"><img src="https://opencollective.com/pnpm/sponsor/1/avatar.svg"></a>
<a href="https://opencollective.com/pnpm/sponsor/2/website" target="_blank"><img src="https://opencollective.com/pnpm/sponsor/2/avatar.svg"></a>
<a href="https://opencollective.com/pnpm/sponsor/3/website" target="_blank"><img src="https://opencollective.com/pnpm/sponsor/3/avatar.svg"></a>
<a href="https://opencollective.com/pnpm/sponsor/4/website" target="_blank"><img src="https://opencollective.com/pnpm/sponsor/4/avatar.svg"></a>
<a href="https://opencollective.com/pnpm/sponsor/5/website" target="_blank"><img src="https://opencollective.com/pnpm/sponsor/5/avatar.svg"></a>
<a href="https://opencollective.com/pnpm/sponsor/6/website" target="_blank"><img src="https://opencollective.com/pnpm/sponsor/6/avatar.svg"></a>
<a href="https://opencollective.com/pnpm/sponsor/7/website" target="_blank"><img src="https://opencollective.com/pnpm/sponsor/7/avatar.svg"></a>
<a href="https://opencollective.com/pnpm/sponsor/8/website" target="_blank"><img src="https://opencollective.com/pnpm/sponsor/8/avatar.svg"></a>
<a href="https://opencollective.com/pnpm/sponsor/9/website" target="_blank"><img src="https://opencollective.com/pnpm/sponsor/9/avatar.svg"></a>

## License

[MIT](https://github.com/pnpm/pnpm/blob/master/LICENSE)
