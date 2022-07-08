import { addDependenciesToPackage } from '@pnpm/core'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { addDistTag, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
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
  await project.hasNot('peer-a')
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

test('don\'t fail on linked package, when peers are auto installed', async () => {
  const pkgManifest = {
    dependencies: {
      linked: 'link:../linked',
    },
  }
  preparePackages([
    {
      location: 'linked',
      package: {
        name: 'linked',
        peerDependencies: {
          'peer-c': '1.0.0',
        },
      },
    },
    {
      location: 'pkg',
      package: pkgManifest,
    },
  ])
  process.chdir('pkg')
  const updatedManifest = await addDependenciesToPackage(pkgManifest, ['peer-b'], await testDefaults({ autoInstallPeers: true }))
  expect(Object.keys(updatedManifest.dependencies ?? {})).toStrictEqual(['linked', 'peer-b'])
})

test('hoist a peer dependency in order to reuse it by other dependencies, when it satisfies them', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['@pnpm/xyz-parent-parent-parent-parent', '@pnpm/xyz-parent-parent-with-xyz'], await testDefaults({ autoInstallPeers: true }))
  const lockfile = await project.readLockfile()
  expect(Object.keys(lockfile.packages)).toStrictEqual([
    '/@pnpm/x/1.0.0',
    '/@pnpm/xyz-parent-parent-parent-parent/1.0.0_e5suan7fvtov6fikg25btc2odi',
    '/@pnpm/xyz-parent-parent-parent/1.0.0_e5suan7fvtov6fikg25btc2odi',
    '/@pnpm/xyz-parent-parent-with-xyz/1.0.0',
    '/@pnpm/xyz-parent-parent/1.0.0_e5suan7fvtov6fikg25btc2odi',
    '/@pnpm/xyz-parent/1.0.0_e5suan7fvtov6fikg25btc2odi',
    '/@pnpm/xyz/1.0.0_e5suan7fvtov6fikg25btc2odi',
    '/@pnpm/y/1.0.0',
    '/@pnpm/z/1.0.0',
  ])
})

test('don\'t hoist a peer dependency when there is a root dependency by that name', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, [
    '@pnpm/xyz-parent-parent-parent-parent',
    '@pnpm/xyz-parent-parent-with-xyz',
    '@pnpm/x@npm:peer-a@1.0.0',
    `http://localhost:${REGISTRY_MOCK_PORT}/@pnpm/y/-/y-2.0.0.tgz`,
  ], await testDefaults({ autoInstallPeers: true }))
  const lockfile = await project.readLockfile()
  expect(Object.keys(lockfile.packages)).toStrictEqual([
    '/@pnpm/x/1.0.0',
    '/@pnpm/xyz-parent-parent-parent-parent/1.0.0_c3hmehglzcfufab5hu6m6d76li',
    '/@pnpm/xyz-parent-parent-parent/1.0.0_c3hmehglzcfufab5hu6m6d76li',
    '/@pnpm/xyz-parent-parent-with-xyz/1.0.0',
    '/@pnpm/xyz-parent-parent/1.0.0_c3hmehglzcfufab5hu6m6d76li',
    '/@pnpm/xyz-parent/1.0.0_c3hmehglzcfufab5hu6m6d76li',
    '/@pnpm/xyz-parent/1.0.0_e5suan7fvtov6fikg25btc2odi',
    '/@pnpm/xyz/1.0.0_c3hmehglzcfufab5hu6m6d76li',
    '/@pnpm/xyz/1.0.0_e5suan7fvtov6fikg25btc2odi',
    '/@pnpm/y/1.0.0',
    '/@pnpm/y/2.0.0',
    '/@pnpm/z/1.0.0',
    '/peer-a/1.0.0',
  ])
})

test('don\'t auto-install a peer dependency, when that dependency is in the root', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, [
    '@pnpm/xyz-parent-parent-parent-parent',
    '@pnpm/x@npm:peer-a@1.0.0',
    `http://localhost:${REGISTRY_MOCK_PORT}/@pnpm/y/-/y-2.0.0.tgz`,
  ], await testDefaults({ autoInstallPeers: true }))
  const lockfile = await project.readLockfile()
  expect(Object.keys(lockfile.packages)).toStrictEqual([
    '/@pnpm/xyz-parent-parent-parent-parent/1.0.0_c3hmehglzcfufab5hu6m6d76li',
    '/@pnpm/xyz-parent-parent-parent/1.0.0_c3hmehglzcfufab5hu6m6d76li',
    '/@pnpm/xyz-parent-parent/1.0.0_c3hmehglzcfufab5hu6m6d76li',
    '/@pnpm/xyz-parent/1.0.0_c3hmehglzcfufab5hu6m6d76li',
    '/@pnpm/xyz/1.0.0_c3hmehglzcfufab5hu6m6d76li',
    '/@pnpm/y/2.0.0',
    '/@pnpm/z/1.0.0',
    '/peer-a/1.0.0',
  ])
})

test('don\'t install the same missing peer dependency twice', async () => {
  await addDistTag({ package: '@pnpm/y', version: '2.0.0', distTag: 'latest' })
  const project = prepareEmpty()
  await addDependenciesToPackage({}, [
    'has-has-y-peer-peer',
  ], await testDefaults({ autoInstallPeers: true }))
  const lockfile = await project.readLockfile()
  expect(Object.keys(lockfile.packages)).toStrictEqual([
    '/@pnpm/y/1.0.0',
    '/has-has-y-peer-peer/1.0.0_c7ewbmm644hn6ztbh6kbjiyhkq',
    '/has-y-peer/1.0.0_@pnpm+y@1.0.0',
  ])
})
