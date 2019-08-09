import { prepareEmpty } from '@pnpm/prepare'
import test from 'jest-t-assert'
import path = require('path')
import rimraf = require('rimraf-then')
import { addDependenciesToPackage, install } from 'supi'
import writeJsonFile = require('write-json-file')
import { testDefaults } from '../utils'

test('repeat install with corrupted `store.json` should work', async t => {
  const project = prepareEmpty(t)

  const opts = await testDefaults()
  const manifest = await addDependenciesToPackage({}, ['is-negative@1.0.0'], opts)

  await rimraf('node_modules')

  // When a package reference is missing from `store.json`
  // we assume that it is not in the store.
  // The package is downloaded and in case there is a folder
  // in the store, it is overwritten.
  await writeJsonFile(path.join(opts.store, '2', 'store.json'), {})

  await install(manifest, opts)

  const m = project.requireModule('is-negative')
  t.ok(m)
})
