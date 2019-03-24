import { tempDir } from '@pnpm/prepare'
import fs = require('mz/fs')
import loadJsonFile from 'load-json-file'
import path = require('path')
import exists = require('path-exists')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { execPnpm } from './utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

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

test('pnpm store add scoped package that uses not the standard registry', async function (t: tape.Test) {
  tempDir(t)
  await fs.writeFile('.npmrc', '@foo:registry=http://localhost:4873/', 'utf8')

  const storeDir = path.resolve('store')

  await execPnpm('store', 'add', '@foo/no-deps@1.0.0', '--registry', 'https://registry.npmjs.org/', '--store', storeDir)

  const pathToCheck = path.join(storeDir, '2', 'localhost+4873', '@foo', 'no-deps', '1.0.0')
  t.ok(await exists(pathToCheck), `@foo/no-deps@1.0.0 is in store (at ${pathToCheck})`)

  const storeIndex = await loadJsonFile(path.join(storeDir, '2', 'store.json'))
  t.deepEqual(
    storeIndex,
    {
      'localhost+4873/@foo/no-deps/1.0.0': [],
    },
    'package has been added to the store index',
  )
})
