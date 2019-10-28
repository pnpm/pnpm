import assertStore from '@pnpm/assert-store'
import { tempDir } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import fs = require('fs')
import loadJsonFile = require('load-json-file')
import path = require('path')
import { storeAdd } from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { testDefaults } from './utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('add packages to the store', async (t: tape.Test) => {
  tempDir(t)
  fs.mkdirSync('_')
  process.chdir('_')

  const opts = await testDefaults()
  const store = assertStore(t, opts.storeDir)

  opts['registry'] = opts.registries!.default // tslint:disable-line
  await storeAdd(['express@4.16.3'], opts)

  // Assert package in file structure
  await store.storeHas('express', '4.16.3')

  // Assert package in store index
  const storeIndex = await loadJsonFile(path.join(opts.storeDir, 'store.json'))
  t.deepEqual(
    storeIndex,
    {
      [`localhost+${REGISTRY_MOCK_PORT}/express/4.16.3`]: [],
    },
    'package has been added to the store index',
  )
})

test('should fail if some packages can not be added', async (t: tape.Test) => {
  tempDir(t)
  fs.mkdirSync('_')
  process.chdir('_')

  let thrown = false
  try {
    await storeAdd(['@pnpm/this-does-not-exist'], await testDefaults())
  } catch (e) {
    thrown = true
    t.equal(e.code, 'ERR_PNPM_STORE_ADD_FAILURE', 'has thrown the correct error code')
    t.equal(e.message, 'Some packages have not been added correctly', 'has thrown the correct error')
  }
  t.ok(thrown, 'has thrown')
})
