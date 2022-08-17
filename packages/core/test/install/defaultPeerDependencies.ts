import path from 'path'
import { prepareEmpty } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { addDependenciesToPackage } from '@pnpm/core'
import deepRequireCwd from 'deep-require-cwd'
import exists from 'path-exists'
import { testDefaults } from '../utils'

test('package with default peer dependency, when auto install peers is on', async () => {
  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
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
    '/dep-of-pkg-with-1-dep/100.0.0',
    '/has-default-peer/1.0.0',
  ])
})

test('package that resolves its own peer dependency', async () => {
  await addDistTag({ package: 'peer-c', version: '1.0.0', distTag: 'latest' })
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['pkg-with-resolved-peer', 'peer-c@2.0.0'], await testDefaults())

  expect(deepRequireCwd(['pkg-with-resolved-peer', 'peer-c', './package.json']).version).toBe('2.0.0')

  expect(await exists(path.resolve('node_modules/.pnpm/pkg-with-resolved-peer@1.0.0_peer-c@2.0.0/node_modules/pkg-with-resolved-peer'))).toBeTruthy()

  const lockfile = await project.readLockfile()

  expect(lockfile.packages['/pkg-with-resolved-peer/1.0.0_peer-c@2.0.0']?.peerDependencies).toStrictEqual({ 'peer-c': '*' })
  expect(lockfile.packages['/pkg-with-resolved-peer/1.0.0_peer-c@2.0.0'].dependencies).toHaveProperty(['peer-c'])
  expect(lockfile.packages['/pkg-with-resolved-peer/1.0.0_peer-c@2.0.0'].optionalDependencies).toHaveProperty(['peer-b'])
})
