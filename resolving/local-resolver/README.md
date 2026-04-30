# @pnpm/resolving.local-resolver

> Resolver for local packages

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/resolving.local-resolver.svg)](https://www.npmjs.com/package/@pnpm/resolving.local-resolver)
<!--/@-->

## Installation

```
pnpm add @pnpm/resolving.local-resolver
```

## Usage

```js
'use strict'
const resolveFromLocal = require('@pnpm/resolving.local-resolver').default

resolveFromLocal({bareSpecifier: './example-package'}, {prefix: process.cwd()})
  .then(resolveResult => console.log(resolveResult))
//> { id: 'link:example-package',
//    normalizedBareSpecifier: 'link:example-package',
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
