import path from 'path'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { type LockfileV7 as Lockfile } from '@pnpm/lockfile-file'
import { prepareEmpty } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { sync as readYamlFile } from 'read-yaml-file'
import { addDependenciesToPackage, install } from '@pnpm/core'
import { testDefaults } from '../utils'

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

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/foobarqar', '@pnpm.e2e/abc-grand-parent-with-c'], testDefaults())

  await Promise.all([
    addDistTag({ package: '@pnpm.e2e/abc-grand-parent-with-c', version: '1.0.1', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/abc-parent-with-ab', version: '1.0.1', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/bar', version: '100.1.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/foobarqar', version: '1.0.1', distTag: 'latest' }),
  ])

  await install(manifest, testDefaults({ update: true, depth: 0 }))

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

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/foobarqar', '@pnpm.e2e/abc-grand-parent-with-c'], testDefaults({ lockfileOnly: true }))

  await Promise.all([
    addDistTag({ package: '@pnpm.e2e/abc-grand-parent-with-c', version: '1.0.1', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/abc-parent-with-ab', version: '1.0.1', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/bar', version: '100.1.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/foobarqar', version: '1.0.1', distTag: 'latest' }),
  ])

  await install(manifest, testDefaults({ update: true, depth: 0 }))

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

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/has-pkg-with-peer-only'], testDefaults())

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
  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/foo'], testDefaults({ lockfileDir }))

  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })

  await install(manifest, testDefaults({ update: true, depth: 0, lockfileDir }))

  const lockfile = readYamlFile<Lockfile>(path.join('..', WANTED_LOCKFILE))

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

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/foobarqar', '@pnpm.e2e/abc-grand-parent-with-c'], testDefaults())

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

  let manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/bar', '@pnpm.e2e/foo'], testDefaults({ hoistPattern: ['*'] }))

  await addDistTag({ package: '@pnpm.e2e/bar', version: '100.1.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })

  manifest = await addDependenciesToPackage(manifest, ['@pnpm.e2e/foo'], testDefaults({ allowNew: false, update: true, hoistPattern: ['*'] }))

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

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/foobarqar', '@pnpm.e2e/abc-grand-parent-with-c'], testDefaults())

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

test('peer dependency is not added to prod deps on update', async () => {
  prepareEmpty()
  const manifest = await install({
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
