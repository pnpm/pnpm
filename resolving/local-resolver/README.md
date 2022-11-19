# @pnpm/local-resolver

> Resolver for local packages

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/local-resolver.svg)](https://www.npmjs.com/package/@pnpm/local-resolver)
<!--/@-->

## Installation

```
pnpm add @pnpm/local-resolver
```

## Usage

```js
'use strict'
const resolveFromLocal = require('@pnpm/local-resolver').default

resolveFromLocal({pref: './example-package'}, {prefix: process.cwd()})
  .then(resolveResult => console.log(resolveResult))
//> { id: 'link:example-package',
//    normalizedPref: 'link:example-package',
//    package:
//     { name: 'foo',
//       version: '1.0.0',
//       readme: '# foo\n',
//       readmeFilename: 'README.md',
//       description: '',
//       _id: 'foo@1.0.0' },
//    resolution: { directory: 'example-package', type: 'directory' }
//    resolvedVia: 'local-filesystem' }
```

## License

MIT
