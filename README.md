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
* [Custom registries](#custom-registries)
* [Preview release](#preview-release)
* [Benchmark](#benchmark)
* [Prior art](#prior-art)
* [License](#license)

## Background

`pnpm` maintains a flat storage of all your dependencies in `node_modules/.store`. They are then symlinked wherever they're needed.
This nets you the benefits of less disk space usage, while keeping your `node_modules` clean.
See [store layout](docs/store-layout.md) for an explanation.

```
.
└─ node_modules/
   ├─ .store/
   │  ├─ chalk@1.1.1/_/
   │  │  └─ node_modules/
   │  │     ├─ ansi-styles      -> ../../../ansi-styles@2.1.0/_
   │  │     └─ has-ansi         -> ../../../has-ansi@2.0.0/_
   │  ├─ ansi-styles@2.1.0/_/
   │  └─ has-ansi@2.0.0/_/
   └─ chalk                     -> .store/chalk@1.1.1/_
```

## Install

Install it via npm.

```
npm install -g pnpm
```

## Usage

Use `pnpm` in place of `npm`. It overrides `pnpm i`, `pnpm install` and some other command, the rest will passthru to `npm`.

```
pnpm install lodash
```

## Custom registries

pnpm uses whatever npm's configured to use as its registry. See: [custom registries](docs/custom-registries.md).

## Preview release

`pnpm` will stay in `<1.0.0` until it's achieved feature parity with `npm install`. See [roadmap](docs/roadmap.md) for details.

## Benchmark

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

## License

[MIT](https://github.com/rstacruz/pnpm/blob/master/LICENSE) © [Rico Sta. Cruz](http://ricostacruz.com) and [contributors]

[contributors]: http://github.com/rstacruz/pnpm/contributors
[ied]: https://github.com/alexanderGugel/ied
