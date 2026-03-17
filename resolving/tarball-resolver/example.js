'use strict'
const resolveFromTarball = require('@pnpm/resolving.tarball-resolver').default

resolveFromTarball({bareSpecifier: 'http://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz'})
  .then(resolveResult => console.log(JSON.stringify(resolveResult, null, 2)))
