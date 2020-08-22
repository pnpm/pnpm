import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage, install } from 'supi'
import promisifyTape from 'tape-promise'
import { testDefaults } from '../utils'
import path = require('path')
import rimraf = require('@zkochan/rimraf')
import tape = require('tape')
import writeJsonFile = require('write-json-file')

const test = promisifyTape(tape)

test('repeat install with corrupted `store.json` should work', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const opts = await testDefaults()
  const manifest = await addDependenciesToPackage({}, ['is-negative@1.0.0'], opts)

  await rimraf('node_modules')

  // When a package reference is missing from `store.json`
  // we assume that it is not in the store.
  // The package is downloaded and in case there is a folder
  // in the store, it is overwritten.
  await writeJsonFile(path.join(opts.storeDir, 'v3/store.json'), {})

  await install(manifest, opts)

  await project.has('is-negative')
})
