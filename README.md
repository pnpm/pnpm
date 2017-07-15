# pnpm-list

> List installed packages in a symlinked \`node_modules\`

<!--@shields('npm', 'travis')-->
[![npm version](https://img.shields.io/npm/v/pnpm-list.svg)](https://www.npmjs.com/package/pnpm-list) [![Build Status](https://img.shields.io/travis/pnpm/pnpm-list/master.svg)](https://travis-ci.org/pnpm/pnpm-list)
<!--/@-->

## Install

Install it via npm.

    npm install pnpm-list

## Usage

<!--@example('./example/index.js')-->
```js
'use strict'
const pnpmList = require('pnpm-list').default

pnpmList(__dirname, [], {depth: 2})
  .then(output => {
    console.log(output)
    //> pnpm-list@0.0.1 /home/zkochan/src/pnpm/pnpm-list/example
    //  └─┬ write-pkg@3.1.0
    //    ├─┬ sort-keys@2.0.0
    //    │ └── is-plain-obj@1.1.0
    //    └─┬ write-json-file@2.2.0
    //      ├── detect-indent@5.0.0
    //      ├── graceful-fs@4.1.11
    //      ├── make-dir@1.0.0
    //      ├── pify@2.3.0
    //      ├── sort-keys@1.1.2
    //      └── write-file-atomic@2.1.0
  })
```
<!--/@-->

## License

[MIT](./LICENSE) © [Zoltan Kochan](https://www.kochan.io/)
