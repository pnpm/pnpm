# pnpm

[![npm version](https://img.shields.io/npm/v/pnpm.svg?maxAge=7200)](https://www.npmjs.com/package/pnpm)
[![Status](https://travis-ci.org/rstacruz/pnpm.svg?branch=master)](https://travis-ci.org/rstacruz/pnpm "See test builds")
[![Windows build status](https://ci.appveyor.com/api/projects/status/i30tealekiaesltb/branch/master?svg=true)](https://ci.appveyor.com/project/zkochan/pnpm/branch/master)

> Performant npm installations

pnpm is a fast implementation of `npm install`. It is loosely based off [ied].

![](docs/images/screencast.gif)

*Read our [contributing guide](CONTRIBUTING.md) if you're looking to contribute.*

Follow the [pnpm Twitter account](https://twitter.com/pnpmjs) for updates.

## Table of Contents

* [Background](#background)
* [Install](#install)
* [Usage](#usage)
  * [Known issues](#known-issues)
* [Benchmark](#benchmark)
* Recipes
  * [Usage in monorepos](docs/recipes/usage-in-monorepos.md)
  * [Continuous Integration](docs/recipes/continuous-integration.md)

## Background

`pnpm` maintains a flat storage of all your dependencies in `~/.pnpm-store`. They are then linked wherever they're needed.
This nets you the benefits of less disk space usage, while keeping your `node_modules` clean.
See [store layout](docs/store-layout.md) for an explanation.

```
=> - a link (also known as a hard link)
-> - a symlink (or junction on Windows)

~/.store
   ├─ chalk/1.1.1/
   |  ├─ index.js
   |  └─ package.json
   ├─ ansi-styles/2.1.0/
   |  ├─ index.js
   |  └─ package.json
   └─ has-ansi/2.0.0/
      ├─ index.js
      └─ package.json
.
└─ node_modules/
   ├─ .resolutions/
   |   ├─ chalk/1.1.1/
   |   |  ├─ node_modules/
   |   |  |  ├─ ansi-styles/ -> ../../ansi-styles/2.1.0/
   |   |  |  └─ has-ansi/    -> ../../has-ansi/2.0.0/
   |   |  ├─ index.js        => ~/.store/chalk/1.1.1/index.js
   |   |  └─ package.json    => ~/.store/chalk/1.1.1/package.json
   |   ├─ has-ansi/2.0.0/
   |   |  ├─ index.js        => ~/.store/has-ansi/2.0.0/index.js
   |   |  └─ package.js      => ~/.store/has-ansi/2.0.0/package.json
   |   └─ ansi-styles/2.1.0/
   |      ├─ index.js        => ~/.store/ansi-styles/2.1.0/index.js
   |      └─ package.js      => ~/.store/ansi-styles/2.1.0/package.json
   └─ chalk/                 -> ./.resolutions/chalk/1.1.1/
```

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

For using globally installed packages, see: [global install](docs/global-install.md).

For using the programmatic API, see: [API](docs/api.md).

### Known issues

Some packages are trying to find and use other packages in `node_modules`.
However, when installed via pnpm, all the dependencies in `node_modules` are
just symlinks. Node's require ignores symlinks when resolving modules and this
causes a lot of issues. Luckily, starting from v6.3.0, Node.js has a CLI option to
preserve symlinks when resolving modules ([--preserve-symlinks](https://nodejs.org/api/cli.html#cli_preserve_symlinks)).

Things to remember:

* pnpm is best used on Node.js >= 6.3.0
* it is recommended to run Node with the `--preserve-symlinks` option. E.g. `node --preserve-symlinks index`. Note: it is important that `--preserve-symlinks` is at the beginning of the command, otherwise it is ignored.
* CLI apps installed with pnpm are configured to correctly work with symlinked dependency trees. If a global package is used to work with a pnpm package, the global package
should be installed via pnpm.

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

## Preview release

`pnpm` will stay in `<1.0.0` until it's achieved feature parity with `npm install`. See [roadmap](https://github.com/rstacruz/pnpm/milestone/1) for details.

## License

[MIT](https://github.com/rstacruz/pnpm/blob/master/LICENSE) © [Rico Sta. Cruz](http://ricostacruz.com) and [contributors]

[contributors]: http://github.com/rstacruz/pnpm/contributors
[ied]: https://github.com/alexanderGugel/ied
