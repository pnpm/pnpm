import { tempDir } from '@pnpm/prepare'
import fs = require('fs')
import loadJsonFile from 'load-json-file'
import path = require('path')
import exists = require('path-exists')
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
  opts['registry'] = opts.registries!.default // tslint:disable-line
  await storeAdd(['express@4.16.3'], opts)

  const pathToCheck = path.join(opts.store, 'localhost+4873', 'express', '4.16.3')
  t.ok(await exists(pathToCheck), `express@4.16.3 is in store (at ${pathToCheck})`)

  const storeIndex = await loadJsonFile(path.join(opts.store, 'store.json'))
  t.deepEqual(
    storeIndex,
    {
      'localhost+4873/express/4.16.3': [],
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
