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
* [Benchmark](#benchmark)
* Recipes
  * [Continuous Integration](docs/recipes/continuous-integration.md)

## Background

`pnpm` maintains a flat storage of all your dependencies in `~/.pnpm-store`. They are then linked wherever they're needed.
This nets you the benefits of **drastically less disk space usage**, while keeping your `node_modules` clean.
See [store layout](docs/store-layout.md) for an explanation.

```
=> - a link (also known as a hard link)
-> - a symlink (or junction on Windows)

~/.pnpm-store
   └─ registry.npmjs.org
      ├─ chalk/1.1.1
      |  ├─ index.js
      |  └─ package.json
      ├─ ansi-styles/2.1.0
      |  ├─ index.js
      |  └─ package.json
      └─ has-ansi/2.0.0
         ├─ index.js
         └─ package.json
.
└─ node_modules
   ├─ chalk                  -> ./.registry.npmjs.org/chalk/1.1.1/node_modules/chalk
   └─ .registry.npmjs.org
       ├─ has-ansi/2.0.0/node_modules
       |  └─ has-ansi
       |     ├─ index.js     => ~/.pnpm-store/registry.npmjs.org/has-ansi/2.0.0/index.js
       |     └─ package.js   => ~/.pnpm-store/registry.npmjs.org/has-ansi/2.0.0/package.json
       |
       ├─ ansi-styles/2.1.0/node_modules
       |  └─ ansi-styles
       |     ├─ index.js     => ~/.pnpm-store/registry.npmjs.org/ansi-styles/2.1.0/index.js
       |     └─ package.js   => ~/.pnpm-store/registry.npmjs.org/ansi-styles/2.1.0/package.json
       |
       └─ chalk/1.1.1/node_modules
          ├─ ansi-styles     -> ../../ansi-styles/2.1.0/node_modules/ansi-styles
          ├─ has-ansi        -> ../../has-ansi/2.0.0/node_modules/has-ansi
          └─ chalk
             ├─ index.js     => ~/.pnpm-store/registry.npmjs.org/chalk/1.1.1/index.js
             └─ package.json => ~/.pnpm-store/registry.npmjs.org/chalk/1.1.1/package.json
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

For using the programmatic API, see: [API](docs/api.md).

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

## License

[MIT](https://github.com/pnpm/pnpm/blob/master/LICENSE)

[contributors]: http://github.com/pnpm/pnpm/contributors
