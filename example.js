'use strict'
const got = require('got')
const createResolveFromNpm = require('@pnpm/npm-resolver').default

const resolveFromNpm = createResolveFromNpm({getJson})

resolveFromNpm({alias: 'is-positive', pref: '1.0.0'}, {
  storePath: '.store',
  registry: 'https://registry.npmjs.org/',
  metaCache: new Map(),
  offline: false,
  getJson,
})
.then(resolveResult => console.log(resolveResult))

function getJson (url, registry) {
  return got(url, {json: true})
    .then(response => response.body)
}
