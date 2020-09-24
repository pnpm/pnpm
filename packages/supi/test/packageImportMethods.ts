import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage } from 'supi'
import promisifyTape from 'tape-promise'
import { testDefaults } from './utils'
import tape = require('tape')

const test = promisifyTape(tape)

test('packageImportMethod can be set to copy', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDependenciesToPackage({}, ['is-negative'], await testDefaults({ fastUnpack: false }, {}, {}, { packageImportMethod: 'copy' }))

  const m = project.requireModule('is-negative')
  t.ok(m, 'is-negative is available with packageImportMethod = copy')
})

test('copy does not fail on package that self-requires itself', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDependenciesToPackage({}, ['requires-itself'], await testDefaults({}, {}, {}, { packageImportMethod: 'copy' }))

  const m = project.requireModule('requires-itself/package.json')
  t.ok(m, 'requires-itself is available with packageImportMethod = copy')

  const lockfile = await project.readLockfile()
  t.deepEqual(lockfile.packages['/requires-itself/1.0.0'].dependencies, { 'is-positive': '1.0.0' })
})
