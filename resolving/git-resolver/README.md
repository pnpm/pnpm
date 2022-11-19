# @pnpm/git-resolver

> Resolver for git-hosted packages

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/git-resolver.svg)](https://www.npmjs.com/package/@pnpm/git-resolver)
<!--/@-->

## Installation

```
pnpm add @pnpm/git-resolver
```

## Usage

<!--@example('./example.js')-->
```js
'use strict'
const createResolveFromNpm = require('@pnpm/git-resolver').default

const resolveFromNpm = createResolveFromNpm({})

resolveFromNpm({
  pref: 'kevva/is-negative#16fd36fe96106175d02d066171c44e2ff83bc055'
})
.then(resolveResult => console.log(JSON.stringify(resolveResult, null, 2)))
//> {
//    "id": "github.com/kevva/is-negative/16fd36fe96106175d02d066171c44e2ff83bc055",
//    "normalizedPref": "github:kevva/is-negative#16fd36fe96106175d02d066171c44e2ff83bc055",
//    "resolution": {
//      "tarball": "https://codeload.github.com/kevva/is-negative/tar.gz/16fd36fe96106175d02d066171c44e2ff83bc055"
//    },
//    "resolvedVia": "git-repository"
//  }
```
<!--/@-->

## License

MIT
