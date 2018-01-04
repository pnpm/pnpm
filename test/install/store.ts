import tape = require('tape')
import promisifyTape from 'tape-promise'
import path = require('path')
import rimraf = require('rimraf-then')
import {prepare, testDefaults} from '../utils'
import writeJsonFile = require('write-json-file')
import {install, installPkgs} from 'supi'

const test = promisifyTape(tape)

test('repeat install with corrupted `store.json` should work', async (t: tape.Test) => {
  const project = prepare(t)

  const opts = await testDefaults()
  await installPkgs(['is-negative@1.0.0'], opts)

  await rimraf('node_modules')

  // When a package reference is missing from `store.json`
  // we assume that it is not in the store.
  // The package is downloaded and in case there is a folder
  // in the store, it is overwritten.
  await writeJsonFile(path.join(opts.store, '2', 'store.json'), {})

  await install(opts)

  const m = project.requireModule('is-negative')
  t.ok(m)
})
