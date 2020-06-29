///<reference path="../../../typings/index.d.ts"/>
import createClient from '@pnpm/client'
import test = require('tape')

test('createClient()', t => {
  const client = createClient({
    metaCache: new Map(),
    rawConfig: { registry: 'https://registry.npmjs.org/' },
    storeDir: '',
  })
  t.equal(typeof client, 'object')
  t.end()
})
