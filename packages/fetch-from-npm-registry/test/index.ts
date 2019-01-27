import test = require('tape')
import createRegClient from 'fetch-from-npm-registry'

test('fetchFromNpmRegistry', async t => {
  const fetchFromNpmRegistry = createRegClient({})
  const res = await fetchFromNpmRegistry('https://registry.npmjs.org/is-positive')
  const metadata = await res.json()
  t.equal(metadata.name, 'is-positive')
  t.notOk(metadata.versions['1.0.0'].scripts)
  t.end()
})

test('fetchFromNpmRegistry fullMetadata', async t => {
  const fetchFromNpmRegistry = createRegClient({fullMetadata: true})
  const res = await fetchFromNpmRegistry('https://registry.npmjs.org/is-positive')
  const metadata = await res.json()
  t.equal(metadata.name, 'is-positive')
  t.ok(metadata.versions['1.0.0'].scripts)
  t.end()
})
