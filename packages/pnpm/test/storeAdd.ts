import loadJsonFile = require('load-json-file')
import path = require('path')
import exists = require('path-exists')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import {tempDir, execPnpm} from './utils'

const test = promisifyTape(tape)

test('pnpm store add express@4.16.3', async function (t: tape.Test) {
  tempDir(t)

  const storeDir = path.resolve('store')

  await execPnpm('store', 'add', 'express@4.16.3', '--store', storeDir)

  const pathToCheck = path.join(storeDir, '2', 'localhost+4873', 'express', '4.16.3')
  t.ok(await exists(pathToCheck), `express@4.16.3 is in store (at ${pathToCheck})`)

  const storeIndex = await loadJsonFile(path.join(storeDir, '2', 'store.json'))
  t.deepEqual(
    storeIndex,
    {
      'localhost+4873/express/4.16.3': [],
    },
    'package has been added to the store index',
  )
})
