import test = require('tape')
import createRegClient from 'fetch-from-npm-registry'

test('fetchFromNpmRegistry', async t => {
  const fetchFromNpmRegistry = createRegClient({})
  const res = await fetchFromNpmRegistry('https://registry.npmjs.org/is-positive')
  const metadata = await res.json()
  t.equal(metadata.name, 'is-positive')
  t.end()
})
