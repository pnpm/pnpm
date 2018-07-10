import loadJsonFile = require('load-json-file')
import path = require('path')

import {
  installPkgs,
  storeAdd,
} from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import {prepare, testDefaults} from './utils'

const test = promisifyTape(tape)

test('add packages to the store', async (t: tape.Test) => {
  const project = prepare(t)

  const opts = await testDefaults()
  // this is needed to initialize the store
  await installPkgs(['is-negative@2.1.0'], opts)

  await storeAdd(['express@4.16.3'], opts)
  await project.storeHas('express', '4.16.3')

  const storeIndex = await loadJsonFile(path.join(opts.store, 'store.json'))
  t.deepEqual(storeIndex['localhost+4873/express/4.16.3'], [])
})
