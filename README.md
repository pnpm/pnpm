# @pnpm/npm-resolver

> Resolver for npm-hosted packages

<!--@shields('npm', 'travis')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/npm-resolver.svg)](https://www.npmjs.com/package/@pnpm/npm-resolver) [![Build Status](https://img.shields.io/travis/pnpm/npm-resolver/master.svg)](https://travis-ci.org/pnpm/npm-resolver)
<!--/@-->

## Install

Install it via npm.

    npm install @pnpm/npm-resolver

## Usage

<!--@example('./example.js')-->
```js
'use strict'
const createResolveFromNpm = require('@pnpm/npm-resolver').default

const resolveFromNpm = createResolveFromNpm({
  metaCache: new Map(),
  store: '.store',
  offline: false,
  rawNpmConfig: {
    registry: 'https://registry.npmjs.org/',
  },
})

resolveFromNpm({alias: 'is-positive', pref: '1.0.0'}, {
  registry: 'https://registry.npmjs.org/',
})
.then(resolveResult => console.log(JSON.stringify(resolveResult, null, 2)))
//> {
//    "id": "registry.npmjs.org/is-positive/1.0.0",
//    "latest": "3.1.0",
//    "package": {
//      "name": "is-positive",
//      "version": "1.0.0",
//      "devDependencies": {
//        "ava": "^0.0.4"
//      },
//      "_hasShrinkwrap": false,
//      "directories": {},
//      "dist": {
//        "shasum": "88009856b64a2f1eb7d8bb0179418424ae0452cb",
//        "tarball": "https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz"
//      },
//      "engines": {
//        "node": ">=0.10.0"
//      }
//    },
//    "resolution": {
//      "integrity": "sha1-iACYVrZKLx632LsBeUGEJK4EUss=",
//      "registry": "https://registry.npmjs.org/",
//      "tarball": "https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz"
//    },
//    "resolvedVia": "npm-registry"
//  }
```
<!--/@-->

## License

[MIT](./LICENSE) © [Zoltan Kochan](https://www.kochan.io/)
