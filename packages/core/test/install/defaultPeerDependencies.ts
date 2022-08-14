import { prepareEmpty } from '@pnpm/prepare'
import {
  addDependenciesToPackage,
} from '@pnpm/core'
import { testDefaults } from '../utils'

/*
 * peer ^1.0.0
 *  prod dep 1.0.0
 * tests to add
 * 1 auto install peers = true. In this case, the package should just work as if there was no prod dep.
 *   installs the latest matching ^1.0.0 (1.1.0)
 *
 * 2 auto install peers = false. There is dep v2 in the root. default peer is not installed
 *
 * 3 auto install peers = false. The prod dep version is installed
 *
*/

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
