![](https://i.imgur.com/qlW1eEG.png)

Fast, disk space efficient package manager:

* **Fast.** Up to 2x faster than the alternatives (see [benchmark](#benchmark)).
* **Efficient.** Files inside `node_modules` are linked from a single content-addressable storage.
* **[Great for monorepos](https://pnpm.js.org/en/workspaces).**
* **Strict.** A package can access only dependencies that are specified in its `package.json`.
* **Deterministic.** Has a lockfile called `pnpm-lock.yaml`.
* **Works everywhere.** Supports Windows, Linux, and macOS.
* **Battle-tested.** Used in production by teams of [all sizes](https://pnpm.js.org/en/users.html) since 2016.
  
To quote the [Rush](https://rushjs.io/) team:

> Microsoft uses pnpm in Rush repos with hundreds of projects and hundreds of PRs per day, and we’ve found it to be very fast and reliable.

[![npm version](https://img.shields.io/npm/v/pnpm.svg)](https://www.npmjs.com/package/pnpm)
![CI](https://github.com/pnpm/pnpm/workflows/CI/badge.svg)
[![Coverage Status](https://coveralls.io/repos/github/pnpm/pnpm/badge.svg?branch=master)](https://coveralls.io/github/pnpm/pnpm?branch=master)
[![Join the chat at Discord](https://img.shields.io/discord/731599538665553971.svg)](https://discord.gg/mThkzAT)
[![OpenCollective](https://opencollective.com/pnpm/backers/badge.svg)](#backers)
[![OpenCollective](https://opencollective.com/pnpm/sponsors/badge.svg)](#sponsors)
[![Twitter Follow](https://img.shields.io/twitter/follow/pnpmjs.svg?style=social&label=Follow)](https://twitter.com/pnpmjs)

## Background

pnpm uses a content-addressable filesystem to store all files from all module directories on a disk.
When using npm or Yarn, if you have 100 projects using lodash, you will have 100 copies of lodash on disk.
With pnpm, lodash will be stored in a content-addressable storage, so:

1. If you depend on different versions of lodash, only the files that differ are added to the store.
  If lodash has 100 files, and a new version has a change only in one of those files,
  `pnpm update` will only add 1 new file to the storage.
1. All the files are saved in a single place on the disk. When packages are installed, their files are linked
  from that single place consuming no additional disk space. Linking is performed using either hard-links or reflinks (copy-on-write).

As a result, you save gigabytes of space on your disk and you have a lot faster installations!
If you'd like more details about the unique `node_modules` structure that pnpm creates and
why it works fine with the Node.js ecosystem, read this small article: [Flat node_modules is not the only way](https://pnpm.js.org/blog/2020/05/27/flat-node-modules-is-not-the-only-way).

## Installation

```
npm install -g pnpm
```

For other installation options [visit our website](https://pnpm.js.org/en/installation).

## Usage

Just use pnpm in place of npm/Yarn. E.g., install dependencies via:

```
pnpm install
```

Also, pnpx instead of npx:

```
pnpx create-react-app my-cool-new-app
```

For more advanced usage, read [pnpm CLI](https://pnpm.js.org/en/pnpm-cli) on our website, or run `pnpm help`.

## Benchmark

pnpm is up to 2x faster than npm and Yarn classic. See all benchmarks [here](https://github.com/pnpm/benchmarks-of-javascript-package-managers).

Benchmarks on an app with lots of dependencies:

![](https://cdn.rawgit.com/pnpm/benchmarks-of-javascript-package-managers/4329296/results/imgs/alotta-files.svg)

## Support

- [Frequently Asked Questions](https://pnpm.js.org/en/faq)
- [Stack Overflow](https://stackoverflow.com/questions/tagged/pnpm)
- [Chat](https://discord.gg/mThkzAT)
- [Twitter](https://twitter.com/pnpmjs)
- [Awesome list](https://github.com/pnpm/awesome-pnpm)

## Contributors

This project exists thanks to all the people who contribute. [[Contribute](../../blob/master/CONTRIBUTING.md)].
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

***

Like this project? Let people know with a [tweet](https://bit.ly/tweet-pnpm).
