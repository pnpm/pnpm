import { prepareEmpty } from '@pnpm/prepare'
import {
  addDependenciesToPackage,
} from '@pnpm/core'
import { testDefaults } from '../utils'

test('package with default peer dependency, when auto install peers is on', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['has-default-peer'], await testDefaults({ autoInstallPeers: true }))

  const lockfile = await project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['/dep-of-pkg-with-1-dep/100.1.0'])
})

test('don\'t install the default peer dependency when it may be resolved from parent packages', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['has-default-peer', 'dep-of-pkg-with-1-dep@101.0.0'], await testDefaults())

  const lockfile = await project.readLockfile()
  expect(Object.keys(lockfile.packages)).toStrictEqual([
    '/dep-of-pkg-with-1-dep/101.0.0',
    '/has-default-peer/1.0.0_ptp3ffmxbab2qqs6nxppnituqi',
  ])
})

test('install the default peer dependency when it cannot be resolved from parent packages', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['has-default-peer'], await testDefaults())

  const lockfile = await project.readLockfile()
  expect(Object.keys(lockfile.packages)).toStrictEqual([
    '/dep-of-pkg-with-1-dep/100.0.0', // TODO: this should be actually something like /dep-of-pkg-with-1-dep/100.0.0_<hash of /has-default-peer/1.0.0>
    '/has-default-peer/1.0.0',
  ])
})
