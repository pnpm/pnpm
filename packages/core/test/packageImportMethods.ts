import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage } from '@pnpm/core'
import { testDefaults } from './utils'

test('packageImportMethod can be set to copy', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['is-negative'], await testDefaults({ fastUnpack: false }, {}, {}, { packageImportMethod: 'copy' }))

  const m = project.requireModule('is-negative')
  expect(m).toBeTruthy() // is-negative is available with packageImportMethod = copy
})

test('copy does not fail on package that self-requires itself', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['requires-itself'], await testDefaults({}, {}, {}, { packageImportMethod: 'copy' }))

  const m = project.requireModule('requires-itself/package.json')
  expect(m).toBeTruthy() // requires-itself is available with packageImportMethod = copy

  const lockfile = await project.readLockfile()
  expect(lockfile.packages['/requires-itself/1.0.0'].dependencies).toStrictEqual({ 'is-positive': '1.0.0' })
})
