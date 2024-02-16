import { WANTED_LOCKFILE } from '@pnpm/constants'
import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage } from '@pnpm/core'
import { testDefaults } from '../utils'

test('fail if installed package does not support the current engine and engine-strict = true', async () => {
  const project = prepareEmpty()

  await expect(
    addDependenciesToPackage({}, ['@pnpm.e2e/not-compatible-with-any-os'], testDefaults({}, {}, {}, {
      engineStrict: true,
    }))
  ).rejects.toThrow()
  project.hasNot('@pnpm.e2e/not-compatible-with-any-os')
  project.storeHasNot('@pnpm.e2e/not-compatible-with-any-os', '1.0.0')
})

test('do not fail if installed package does not support the current engine and engine-strict = false', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/not-compatible-with-any-os'], testDefaults({
    engineStrict: false,
  }))

  project.has('@pnpm.e2e/not-compatible-with-any-os')
  project.storeHas('@pnpm.e2e/not-compatible-with-any-os', '1.0.0')

  const lockfile = project.readLockfile()
  expect(lockfile.packages['/@pnpm.e2e/not-compatible-with-any-os@1.0.0'].os).toStrictEqual(['this-os-does-not-exist'])
})

test('do not fail if installed package requires the node version that was passed in and engine-strict = true', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/for-legacy-node'], testDefaults({
    engineStrict: true,
    nodeVersion: '0.10.0',
  }))

  project.has('@pnpm.e2e/for-legacy-node')
  project.storeHas('@pnpm.e2e/for-legacy-node', '1.0.0')

  const lockfile = project.readLockfile()
  expect(lockfile.packages['/@pnpm.e2e/for-legacy-node@1.0.0'].engines).toStrictEqual({ node: '0.10' })
})

test(`save cpu field to ${WANTED_LOCKFILE}`, async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/has-cpu-specified'], testDefaults())

  const lockfile = project.readLockfile()

  expect(
    lockfile.packages['/@pnpm.e2e/has-cpu-specified@1.0.0'].cpu
  ).toStrictEqual(
    ['x64', 'ia32']
  )
})

test(`engines field is not added to ${WANTED_LOCKFILE} when "node": "*" is in "engines" field`, async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['jsonify@0.0.0'], testDefaults())

  const lockfile = project.readLockfile()

  expect(lockfile.packages['/jsonify@0.0.0']).not.toHaveProperty(['engines'])
})
