import { WANTED_LOCKFILE } from '@pnpm/constants'
import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage } from '@pnpm/core'
import { testDefaults } from '../utils'

test('fail if installed package does not support the current engine and engine-strict = true', async () => {
  const project = prepareEmpty()

  await expect(
    addDependenciesToPackage({}, ['@pnpm.e2e/not-compatible-with-any-os'], await testDefaults({}, {}, {}, {
      engineStrict: true,
    }))
  ).rejects.toThrow()
  await project.hasNot('@pnpm.e2e/not-compatible-with-any-os')
  await project.storeHasNot('@pnpm.e2e/not-compatible-with-any-os', '1.0.0')
})

test('do not fail if installed package does not support the current engine and engine-strict = false', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/not-compatible-with-any-os'], await testDefaults({
    engineStrict: false,
  }))

  await project.has('@pnpm.e2e/not-compatible-with-any-os')
  await project.storeHas('@pnpm.e2e/not-compatible-with-any-os', '1.0.0')

  const lockfile = await project.readLockfile()
  expect(lockfile.packages['/@pnpm.e2e/not-compatible-with-any-os@1.0.0'].os).toStrictEqual(['this-os-does-not-exist'])
})

test('do not fail if installed package requires the node version that was passed in and engine-strict = true', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/for-legacy-node'], await testDefaults({
    engineStrict: true,
    nodeVersion: '0.10.0',
  }))

  await project.has('@pnpm.e2e/for-legacy-node')
  await project.storeHas('@pnpm.e2e/for-legacy-node', '1.0.0')

  const lockfile = await project.readLockfile()
  expect(lockfile.packages['/@pnpm.e2e/for-legacy-node@1.0.0'].engines).toStrictEqual({ node: '0.10' })
})

test(`save cpu field to ${WANTED_LOCKFILE}`, async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/has-cpu-specified'], await testDefaults())

  const lockfile = await project.readLockfile()

  expect(
    lockfile.packages['/@pnpm.e2e/has-cpu-specified@1.0.0'].cpu
  ).toStrictEqual(
    ['x64', 'ia32']
  )
})

test(`engines field is not added to ${WANTED_LOCKFILE} when "node": "*" is in "engines" field`, async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['jsonify@0.0.0'], await testDefaults())

  const lockfile = await project.readLockfile()

  expect(lockfile.packages['/jsonify@0.0.0']).not.toHaveProperty(['engines'])
})
