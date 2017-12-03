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
})

resolveFromNpm({alias: 'is-positive', pref: '1.0.0'}, {
  storePath: '.store',
  registry: 'https://registry.npmjs.org/',
  offline: false,
})
.then(resolveResult => console.log(resolveResult))
//> { id: 'registry.npmjs.org/is-positive/1.0.0',
//    latest: '3.1.0',
//    package: 
//     { name: 'is-positive',
//       version: '1.0.0',
//       description: 'Test if a number is positive',
//       license: 'MIT',
//       repository: 
//        { type: 'git',
//          url: 'git+https://github.com/kevva/is-positive.git' },
//       author: 
//        { name: 'Kevin Martensson',
//          email: 'kevinmartensson@gmail.com',
//          url: 'github.com/kevva' },
//       engines: { node: '>=0.10.0' },
//       scripts: { test: 'node test.js' },
//       files: [ 'index.js' ],
//       keywords: [ 'number', 'positive', 'test' ],
//       devDependencies: { ava: '^0.0.4' },
//       gitHead: '1187a61f2e18cf7c11c23d61a1bd52b9fa6a5fdf',
//       bugs: { url: 'https://github.com/kevva/is-positive/issues' },
//       homepage: 'https://github.com/kevva/is-positive#readme',
//       _id: 'is-positive@1.0.0',
//       _shasum: '88009856b64a2f1eb7d8bb0179418424ae0452cb',
//       _from: '.',
//       _npmVersion: '2.11.1',
//       _nodeVersion: '2.0.1',
//       _npmUser: { name: 'kevva', email: 'kevinmartensson@gmail.com' },
//       maintainers: [ [Object] ],
//       dist: 
//        { shasum: '88009856b64a2f1eb7d8bb0179418424ae0452cb',
//          tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz' },
//       directories: {} },
//    resolution: 
//     { integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
//       registry: 'https://registry.npmjs.org/',
//       tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz' } }
```
<!--/@-->

## License

[MIT](./LICENSE) © [Zoltan Kochan](https://www.kochan.io/)
