# pnpm

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

pnpm follows whatever is configured as npm registries. To use a custom registry, use `npm config`:

```sh
# updates ~/.npmrc
npm config set registry http://npmjs.eu
```

Or to use it for just one command, use environment variables:

```
env npm_registry=http://npmjs.eu pnpm install
```

Private registries are supported, as well.

```sh
npm config set @mycompany:registry https://npm.mycompany.com
pnpm install @mycompany/foo
```

## Preview release

`pnpm` will stay in `<1.0.0` until it's achieved feature parity with `npm install`.

- [ ] `pnpm install`
  - [x] npm packages
  - [x] install from packages (`npm i`)
  - [x] @scoped packages (`npm i @rstacruz/tap-spec`)
  - [x] tarball release packages (`npm i http://foo.com/tar.tgz`)
  - [x] compiled packages (`npm i node-sass`)
  - [x] bundled dependencies (`npm i fsevents@1.0.6`)
  - [ ] git-hosted packages (`npm i rstacruz/scourjs`)
  - [ ] optional dependencies (`npm i escodegen@1.8.0` wants `source-map@~0.2.0`)
  - [ ] file packages (`npm i file:../path`)
  - [x] bin executables
  - [ ] `--global` installs
  - [x] `--save` (et al)
- [ ] `pnpm uninstall`
- [x] `pnpm ls`

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

[ied][] is built on a very similar premise. `pnpm` takes huge inspiration from ied.

Unlike ied, however:

- `pnpm` will eventually be made to support a globally-shared store so you can keep all your npm modules in one place. With this goal in mind, `pnpm` also doesn't care much about `npm@3`'s flat dependency tree style.
- pnpm also supports circular dependencies.
- pnpm aims to achieve compatibility with npm utilities (eg, shrinkwrap), and so deviates from ied's store schema (see [§ Design](#design)).

[ied]: https://github.com/alexanderGugel/ied

## Will pnpm replace npm?

**No!** pnpm is not a _replacement_ for npm; rather, think of it as a _supplement_ to npm.

It's simply a rewrite of the `npm install` command that uses an alternate way to store your modules. It won't reimplement other things npm is used for (publishing, node_modules management, and so on).

## Limitations

- Windows is [not fully supported](https://github.com/rstacruz/pnpm/issues/6) (yet).
- You can't install from [shrinkwrap][] (yet).
- Things not ticked off in the [to do list](#preview-release) are obviously not feature-complete.

Got an idea for workarounds for these issues? [Share them.](https://github.com/rstacruz/pnpm/issues/new)

[shrinkwrap]: https://docs.npmjs.com/cli/shrinkwrap
[npm ls]: https://docs.npmjs.com/cli/ls
[npm prune]: https://docs.npmjs.com/cli/prune
[npm dedupe]: https://docs.npmjs.com/cli/dedupe

## Thanks

**pnpm** © 2016+, Rico Sta. Cruz. Released under the [MIT] License.<br>
Authored and maintained by Rico Sta. Cruz with help from contributors ([list][contributors]).

> [ricostacruz.com](http://ricostacruz.com) &nbsp;&middot;&nbsp;
> GitHub [@rstacruz](https://github.com/rstacruz) &nbsp;&middot;&nbsp;
> Twitter [@rstacruz](https://twitter.com/rstacruz)

[MIT]: http://mit-license.org/
[contributors]: http://github.com/rstacruz/pnpm/contributors
