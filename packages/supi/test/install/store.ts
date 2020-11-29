import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage, install } from 'supi'
import { testDefaults } from '../utils'
import path = require('path')
import rimraf = require('@zkochan/rimraf')
import writeJsonFile = require('write-json-file')

test('repeat install with corrupted `store.json` should work', async () => {
  const project = prepareEmpty()

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
