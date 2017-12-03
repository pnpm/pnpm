'use strict'
const createResolveFromNpm = require('@pnpm/npm-resolver').default

const resolveFromNpm = createResolveFromNpm({
  metaCache: new Map(),
  store: '.store',
  offline: false,
})

resolveFromNpm({alias: 'is-positive', pref: '1.0.0'}, {
  registry: 'https://registry.npmjs.org/',
})
.then(resolveResult => console.log(resolveResult))
