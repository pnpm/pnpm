# @pnpm/resolving.git-resolver

> Resolver for git-hosted packages

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/resolving.git-resolver.svg)](https://npmx.dev/package/@pnpm/resolving.git-resolver)
<!--/@-->

## Installation

```
pnpm add @pnpm/resolving.git-resolver
```

## Usage

<!--@example('./example.js')-->
```js
'use strict'
const createResolveFromNpm = require('@pnpm/resolving.git-resolver').default

const resolveFromNpm = createResolveFromNpm({})

resolveFromNpm({
  bareSpecifier: 'kevva/is-negative#16fd36fe96106175d02d066171c44e2ff83bc055'
})
.then(resolveResult => console.log(JSON.stringify(resolveResult, null, 2)))
//> {
//    "id": "github.com/kevva/is-negative/16fd36fe96106175d02d066171c44e2ff83bc055",
//    "normalizedBareSpecifier": "github:kevva/is-negative#16fd36fe96106175d02d066171c44e2ff83bc055",
//    "resolution": {
//      "tarball": "https://codeload.github.com/kevva/is-negative/tar.gz/16fd36fe96106175d02d066171c44e2ff83bc055"
//    },
//    "resolvedVia": "git-repository"
//  }
```
<!--/@-->

## License

MIT
