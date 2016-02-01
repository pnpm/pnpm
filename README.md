# pnpm

<!-- {.massive-header.-with-tagline} -->

> **P**erformant **npm**

pnpm is a fast implementation of `npm install`. It is loosely based off [ied].

![](https://raw.githubusercontent.com/rstacruz/pnpm/gh-pages/screencast.gif)

[![npm version](https://badge.fury.io/js/pnpm.js.svg)](https://badge.fury.io/js/pnpm.js)
[![Status](https://travis-ci.org/rstacruz/pnpm.svg?branch=master)](https://travis-ci.org/rstacruz/pnpm "See test builds")

## Install

Install it via npm.

```
npm install -g pnpm.js
```

Use `pnpm` in place of `npm`. It overrides `pnpm i` and `pnpm install`—all other commands will passthru to `npm`.

```
pnpm install lodash
```

## Custom registries

pnpm uses whatever npm's configured to use as its registry. See: [custom registries](docs/custom-registries.md).

## Preview release

`pnpm` will stay in `<1.0.0` until it's achieved feature parity with `npm install`. See [roadmap](docs/roadmap.md) for details.

## Design

`pnpm` maintains a flat storage of all your dependencies in `node_modules/.store`. They are then symlinked whereever they're needed.
This is like `npm@2`'s recursive module handling (without the disk space bloat), and like `npm@3`s flat dependency tree (except with each module being predictably atomic).
To illustrate, an installation of [chalk][]@1.1.1 may look like this:

```
.
└─ node_modules/
   ├─ .store/
   │  ├─ chalk@1.1.1/_/
   │  │  └─ node_modules/
   │  │     ├─ ansi-styles      -> ../../../ansi-styles@2.1.0/_
   │  │     ├─ has-ansi         -> ../../../has-ansi@2.0.0/_
   │  │     └─ supports-color   -> ../../../supports-color@2.0.0/_
   │  ├─ ansi-styles@2.1.0/_/
   │  ├─ has-ansi@2.0.0/_/
   │  └─ supports-color@2.0.0/_/
   └─ chalk                     -> .store/chalk@1.1.1/_
```

The intermediate `_` directories are needed to hide `node_modules` from npm utilities like `npm ls`, `npm prune`, `npm shrinkwrap` and so on. The name `_` is chosen because it helps make stack traces readable.

[chalk]: https://github.com/chalk/chalk

## Benchmark

```
time npm i babel-preset-es2015 browserify chalk debug minimist mkdirp
    66.15 real        15.60 user         3.54 sys

time pnpm i babel-preset-es2015 browserify chalk debug minimist mkdirp
    11.04 real         6.85 user         2.85 sys
```

## Prior art

* [Compared to ied](docs/vs-npm.md)
* [Compared to npm](docs/vs-npm.md)

## Thanks

**pnpm** © 2016+, Rico Sta. Cruz. Released under the [MIT] License.<br>
Authored and maintained by Rico Sta. Cruz with help from contributors ([list][contributors]).

> [ricostacruz.com](http://ricostacruz.com) &nbsp;&middot;&nbsp;
> GitHub [@rstacruz](https://github.com/rstacruz) &nbsp;&middot;&nbsp;
> Twitter [@rstacruz](https://twitter.com/rstacruz)

[MIT]: http://mit-license.org/
[contributors]: http://github.com/rstacruz/pnpm/contributors
