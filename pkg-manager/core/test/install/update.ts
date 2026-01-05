import path from 'path'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { type LockfileFile } from '@pnpm/lockfile.fs'
import { prepareEmpty } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { sync as readYamlFile } from 'read-yaml-file'
import { addDependenciesToPackage, install } from '@pnpm/core'
import { testDefaults } from '../utils/index.js'

test('preserve subdeps on update', async () => {
  const project = prepareEmpty()

  await Promise.all([
    addDistTag({ package: '@pnpm.e2e/abc-grand-parent-with-c', version: '1.0.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/abc-parent-with-ab', version: '1.0.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/bar', version: '100.0.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/foo', version: '100.0.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/foobarqar', version: '1.0.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/peer-c', version: '1.0.0', distTag: 'latest' }),
  ])

  const { updatedManifest } = await addDependenciesToPackage({}, ['@pnpm.e2e/foobarqar', '@pnpm.e2e/abc-grand-parent-with-c'], testDefaults())

  await Promise.all([
    addDistTag({ package: '@pnpm.e2e/abc-grand-parent-with-c', version: '1.0.1', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/abc-parent-with-ab', version: '1.0.1', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/bar', version: '100.1.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/foobarqar', version: '1.0.1', distTag: 'latest' }),
  ])

  await install(updatedManifest, testDefaults({ update: true, depth: 0 }))

  const lockfile = project.readLockfile()

  expect(lockfile.snapshots).toBeTruthy()
  expect(lockfile.snapshots).toHaveProperty(['@pnpm.e2e/abc-parent-with-ab@1.0.0(@pnpm.e2e/peer-c@1.0.0)'])
  expect(lockfile.snapshots).toHaveProperty(['@pnpm.e2e/foobarqar@1.0.1'])
  expect(lockfile.snapshots['@pnpm.e2e/foobarqar@1.0.1'].dependencies).toStrictEqual({
    '@pnpm.e2e/bar': '100.0.0',
    '@pnpm.e2e/foo': '100.0.0',
    '@pnpm.e2e/qar': '100.0.0',
  })
})

test('preserve subdeps on update when no node_modules is present', async () => {
  const project = prepareEmpty()

  await Promise.all([
    addDistTag({ package: '@pnpm.e2e/abc-grand-parent-with-c', version: '1.0.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/abc-parent-with-ab', version: '1.0.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/bar', version: '100.0.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/foo', version: '100.0.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/foobarqar', version: '1.0.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/peer-c', version: '1.0.0', distTag: 'latest' }),
  ])

  const { updatedManifest } = await addDependenciesToPackage({}, ['@pnpm.e2e/foobarqar', '@pnpm.e2e/abc-grand-parent-with-c'], testDefaults({ lockfileOnly: true }))

  await Promise.all([
    addDistTag({ package: '@pnpm.e2e/abc-grand-parent-with-c', version: '1.0.1', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/abc-parent-with-ab', version: '1.0.1', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/bar', version: '100.1.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/foobarqar', version: '1.0.1', distTag: 'latest' }),
  ])

  await install(updatedManifest, testDefaults({ update: true, depth: 0 }))

  const lockfile = project.readLockfile()

  expect(lockfile.packages).toBeTruthy()
  expect(lockfile.snapshots).toHaveProperty(['@pnpm.e2e/abc-parent-with-ab@1.0.0(@pnpm.e2e/peer-c@1.0.0)']) // preserve version of package that has resolved peer deps
  expect(lockfile.snapshots).toHaveProperty(['@pnpm.e2e/foobarqar@1.0.1'])
  expect(lockfile.snapshots['@pnpm.e2e/foobarqar@1.0.1'].dependencies).toStrictEqual({
    '@pnpm.e2e/bar': '100.0.0',
    '@pnpm.e2e/foo': '100.0.0',
    '@pnpm.e2e/qar': '100.0.0',
  })
})

test('update does not fail when package has only peer dependencies', async () => {
  prepareEmpty()

  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['@pnpm.e2e/has-pkg-with-peer-only'], testDefaults())

  await install(manifest, testDefaults({ update: true, depth: Infinity }))
})

test('update does not install the package if it is not present in package.json', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['is-positive'], testDefaults({
    allowNew: false,
    update: true,
  }))

  project.hasNot('is-positive')
})

test('update dependency when external lockfile directory is used', async () => {
  prepareEmpty()

  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.0.0', distTag: 'latest' })

  const lockfileDir = path.resolve('..')
  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['@pnpm.e2e/foo'], testDefaults({ lockfileDir }))

  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })

  await install(manifest, testDefaults({ update: true, depth: 0, lockfileDir }))

  const lockfile = readYamlFile<LockfileFile>(path.join('..', WANTED_LOCKFILE))

  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/foo@100.1.0'])
})

// Covers https://github.com/pnpm/pnpm/issues/2191
test('preserve subdeps when installing on a package that has one dependency spec changed in the manifest', async () => {
  const project = prepareEmpty()

  await Promise.all([
    addDistTag({ package: '@pnpm.e2e/abc-grand-parent-with-c', version: '1.0.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/abc-parent-with-ab', version: '1.0.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/bar', version: '100.0.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/foo', version: '100.0.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/foobarqar', version: '1.0.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/peer-c', version: '1.0.0', distTag: 'latest' }),
  ])

  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['@pnpm.e2e/foobarqar', '@pnpm.e2e/abc-grand-parent-with-c'], testDefaults())

  manifest.dependencies!['@pnpm.e2e/foobarqar'] = '^1.0.1'

  await Promise.all([
    addDistTag({ package: '@pnpm.e2e/abc-parent-with-ab', version: '1.0.1', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/bar', version: '100.1.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/foobarqar', version: '1.0.1', distTag: 'latest' }),
  ])

  await install(manifest, testDefaults())

  const lockfile = project.readLockfile()

  expect(lockfile.snapshots).toHaveProperty(['@pnpm.e2e/abc-parent-with-ab@1.0.0(@pnpm.e2e/peer-c@1.0.0)']) // preserve version of package that has resolved peer deps
  expect(lockfile.snapshots).toHaveProperty(['@pnpm.e2e/foobarqar@1.0.1'])
  expect(lockfile.snapshots['@pnpm.e2e/foobarqar@1.0.1'].dependencies).toStrictEqual({
    '@pnpm.e2e/bar': '100.0.0',
    '@pnpm.e2e/foo': '100.0.0',
    '@pnpm.e2e/qar': '100.0.0',
  })
})

// Covers https://github.com/pnpm/pnpm/issues/2226
test('update only the packages that were requested to be updated when hoisting is on', async () => {
  const project = prepareEmpty()

  await addDistTag({ package: '@pnpm.e2e/bar', version: '100.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.0.0', distTag: 'latest' })

  let { updatedManifest: manifest } = await addDependenciesToPackage({}, ['@pnpm.e2e/bar', '@pnpm.e2e/foo'], testDefaults({ hoistPattern: ['*'] }))

  await addDistTag({ package: '@pnpm.e2e/bar', version: '100.1.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })

  manifest = (await addDependenciesToPackage(manifest, ['@pnpm.e2e/foo'], testDefaults({ allowNew: false, update: true, hoistPattern: ['*'] }))).updatedManifest

  expect(manifest.dependencies).toStrictEqual({ '@pnpm.e2e/bar': '^100.0.0', '@pnpm.e2e/foo': '^100.1.0' })

  const lockfile = project.readLockfile()
  expect(Object.keys(lockfile.packages)).toStrictEqual(['@pnpm.e2e/bar@100.0.0', '@pnpm.e2e/foo@100.1.0'])
})

test('update only the specified package', async () => {
  const project = prepareEmpty()

  await Promise.all([
    addDistTag({ package: '@pnpm.e2e/abc-grand-parent-with-c', version: '1.0.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/abc-parent-with-ab', version: '1.0.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/bar', version: '100.0.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/foo', version: '100.0.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/foobarqar', version: '1.0.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/peer-c', version: '1.0.0', distTag: 'latest' }),
  ])

  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['@pnpm.e2e/foobarqar', '@pnpm.e2e/abc-grand-parent-with-c'], testDefaults())

  await Promise.all([
    addDistTag({ package: '@pnpm.e2e/abc-grand-parent-with-c', version: '1.0.1', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/abc-parent-with-ab', version: '1.0.1', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/bar', version: '100.1.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/foobarqar', version: '1.0.1', distTag: 'latest' }),
  ])

  await install(manifest, testDefaults({
    depth: Infinity,
    update: true,
    updateMatching: (pkgName: string) => pkgName === '@pnpm.e2e/foo',
  }))

  const lockfile = project.readLockfile()

  expect(lockfile.snapshots).toHaveProperty(['@pnpm.e2e/abc-parent-with-ab@1.0.0(@pnpm.e2e/peer-c@1.0.0)'])
  expect(lockfile.snapshots).toHaveProperty(['@pnpm.e2e/foobarqar@1.0.0'])
  expect(lockfile.snapshots['@pnpm.e2e/foobarqar@1.0.0'].dependencies).toStrictEqual({
    '@pnpm.e2e/bar': '100.0.0',
    '@pnpm.e2e/foo': '100.1.0',
    'is-positive': '3.1.0',
  })
})

test.each([false, true])('update only the specified package with --lockfile-only=%p', async (lockfileOnly) => {
  const project = prepareEmpty()

  await Promise.all([
    addDistTag({ package: '@pnpm.e2e/bar', version: '100.0.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/foo', version: '100.0.0', distTag: 'latest' }),
  ])

  const { updatedManifest: manifest } = await addDependenciesToPackage({}, [
    '@pnpm.e2e/bar',
    '@pnpm.e2e/foo',
    // Ensure aliases also stay on the same version.
    'bar-alias@npm:@pnpm.e2e/bar',
  ], testDefaults())

  await Promise.all([
    addDistTag({ package: '@pnpm.e2e/bar', version: '100.1.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' }),
  ])

  await install(manifest, testDefaults({
    depth: Infinity,
    update: true,
    updateMatching: (pkgName: string) => pkgName === '@pnpm.e2e/foo',

    // This test specifically tests this flag.
    lockfileOnly,
  }))

  const lockfile = project.readLockfile()
  expect(lockfile.snapshots).toStrictEqual({
    '@pnpm.e2e/bar@100.0.0': expect.anything(),
    '@pnpm.e2e/foo@100.1.0': expect.anything(),
  })
})

test('peer dependency is not added to prod deps on update', async () => {
  prepareEmpty()
  const { updatedManifest: manifest } = await install({
    peerDependencies: {
      'is-positive': '^3.0.0',
    },
  }, testDefaults({ autoInstallPeers: true, update: true, depth: 0 }))
  expect(manifest).toStrictEqual({
    peerDependencies: {
      'is-positive': '^3.0.0',
    },
  })
})

// Covers https://github.com/pnpm/pnpm/issues/9900
test('peer dependencies are updated with pnpm upgrade --latest when autoInstallPeers is true', async () => {
  await addDistTag({ package: '@pnpm.e2e/foo', version: '1.0.0', distTag: 'latest' })

  const project = prepareEmpty()

  const manifest = {
    name: 'test-pkg',
    version: '1.0.0',
    peerDependencies: {
      '@pnpm.e2e/foo': '^1.0.0',
    },
  }

  await install(manifest, testDefaults({ autoInstallPeers: true }))

  let lockfile = project.readLockfile()
  expect(lockfile.importers?.['.']?.dependencies?.['@pnpm.e2e/foo'].version).toBe('1.0.0')

  await addDistTag({ package: '@pnpm.e2e/foo', version: '1.3.0', distTag: 'latest' })

  await addDependenciesToPackage(manifest, ['@pnpm.e2e/foo'], testDefaults({
    allowNew: false,
    autoInstallPeers: true,
    update: true,
    updateToLatest: true,
  }))

  lockfile = project.readLockfile()

  expect(lockfile.importers?.['.']?.dependencies?.['@pnpm.e2e/foo'].version).toBe('1.3.0')
})
