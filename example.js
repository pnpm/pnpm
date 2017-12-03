'use strict'
const createResolveFromNpm = require('@pnpm/npm-resolver').default

const resolveFromNpm = createResolveFromNpm({})

resolveFromNpm({alias: 'is-positive', pref: '1.0.0'}, {
  storePath: '.store',
  registry: 'https://registry.npmjs.org/',
  offline: false,
})
.then(resolveResult => console.log(resolveResult))
