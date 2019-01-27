'use strict'
const createFetcher = require('fetch-from-npm-registry').default

const fetchFromNpmRegistry = createFetcher({userAgent: 'fetch-from-npm-registry'})

fetchFromNpmRegistry('https://registry.npmjs.org/is-positive')
  .then(res => res.json())
  .then(metadata => console.log(JSON.stringify(metadata.versions['1.0.0'], null, 2)))
