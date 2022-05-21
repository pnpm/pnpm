import { addDependenciesToPackage } from '@pnpm/core'
import { prepareEmpty } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { testDefaults } from '../utils'

test('auto install non-optional peer dependencies', async () => {
  await addDistTag({ package: 'peer-a', version: '1.0.0', distTag: 'latest' })
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['abc-optional-peers@1.0.0'], await testDefaults({ autoInstallPeers: true }))
  const lockfile = await project.readLockfile()
  expect(Object.keys(lockfile.packages)).toStrictEqual([
    '/abc-optional-peers/1.0.0_peer-a@1.0.0',
    '/peer-a/1.0.0',
  ])
})

test('auto install the common peer dependency', async () => {
  await addDistTag({ package: 'peer-c', version: '1.0.1', distTag: 'latest' })
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['wants-peer-c-1', 'wants-peer-c-1.0.0'], await testDefaults({ autoInstallPeers: true }))
  const lockfile = await project.readLockfile()
  expect(Object.keys(lockfile.packages)).toStrictEqual([
    '/peer-c/1.0.0',
    '/wants-peer-c-1.0.0/1.0.0_peer-c@1.0.0',
    '/wants-peer-c-1/1.0.0_peer-c@1.0.0',
  ])
})

test('do not auto install when there is no common peer dependency range intersection', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['wants-peer-c-1', 'wants-peer-c-2'], await testDefaults({ autoInstallPeers: true }))
  const lockfile = await project.readLockfile()
  expect(Object.keys(lockfile.packages)).toStrictEqual([
    '/wants-peer-c-1/1.0.0',
    '/wants-peer-c-2/1.0.0',
  ])
})
