'use strict'
const createFetcher = require('@pnpm/tarball-fetcher').default

process.chdir(__dirname)

const registry = 'https://registry.npmjs.org/'
const fetch = createFetcher({
  registry,
  rawNpmConfig: {
    registry,
  },
})

const resolution = {
  tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
}
fetch.tarball(resolution, 'dist/unpacked', {
  cachedTarballLocation: 'dist/cache.tgz',
  prefix: process.cwd(),
})
.then(index => console.log(Object.keys(index)))
.catch(err => {
  console.error(err)
  process.exit(1)
})
