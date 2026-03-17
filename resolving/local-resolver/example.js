'use strict'
const resolveFromLocal = require('@pnpm/resolving.local-resolver').default

resolveFromLocal({bareSpecifier: './example-package'}, {prefix: process.cwd()})
  .then(resolveResult => console.log(resolveResult))
