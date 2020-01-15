'use strict'
const createResolveFromNpm = require('@pnpm/npm-resolver').default

const resolveFromNpm = createResolveFromNpm({
  metaCache: new Map(),
  storeDir: '.store',
  offline: false,
  rawConfig: {
    registry: 'https://registry.npmjs.org/',
  },
})

resolveFromNpm({alias: 'regenerator/runtime', pref: '1.0.0'}, {
  registry: 'https://registry.npmjs.org/',
})
.then(resolveResult => console.log(JSON.stringify(resolveResult, null, 2)))
