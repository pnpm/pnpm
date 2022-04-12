import path from 'path'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { Lockfile } from '@pnpm/lockfile-file'
import { prepareEmpty } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import readYamlFile from 'read-yaml-file'
import { addDependenciesToPackage, install } from '@pnpm/core'
import { testDefaults } from '../utils'

test('preserve subdeps on update', async () => {
  const project = prepareEmpty()

  await Promise.all([
    addDistTag({ package: 'abc-grand-parent-with-c', version: '1.0.0', distTag: 'latest' }),
    addDistTag({ package: 'abc-parent-with-ab', version: '1.0.0', distTag: 'latest' }),
    addDistTag({ package: 'bar', version: '100.0.0', distTag: 'latest' }),
    addDistTag({ package: 'foo', version: '100.0.0', distTag: 'latest' }),
    addDistTag({ package: 'foobarqar', version: '1.0.0', distTag: 'latest' }),
    addDistTag({ package: 'peer-c', version: '1.0.0', distTag: 'latest' }),
  ])

  const manifest = await addDependenciesToPackage({}, ['foobarqar', 'abc-grand-parent-with-c'], await testDefaults())

  await Promise.all([
    addDistTag({ package: 'abc-grand-parent-with-c', version: '1.0.1', distTag: 'latest' }),
    addDistTag({ package: 'abc-parent-with-ab', version: '1.0.1', distTag: 'latest' }),
    addDistTag({ package: 'bar', version: '100.1.0', distTag: 'latest' }),
    addDistTag({ package: 'foo', version: '100.1.0', distTag: 'latest' }),
    addDistTag({ package: 'foobarqar', version: '1.0.1', distTag: 'latest' }),
  ])

  await install(manifest, await testDefaults({ update: true, depth: 0 }))

  const lockfile = await project.readLockfile()

  expect(lockfile.packages).toBeTruthy()
  expect(lockfile.packages).toHaveProperty(['/abc-parent-with-ab/1.0.0_peer-c@1.0.0'])
  expect(lockfile.packages).toHaveProperty(['/foobarqar/1.0.1'])
  expect(lockfile.packages['/foobarqar/1.0.1'].dependencies).toStrictEqual({
    bar: '100.0.0',
    foo: '100.0.0',
    qar: '100.0.0',
  })
})

test('preserve subdeps on update when no node_modules is present', async () => {
  const project = prepareEmpty()

  await Promise.all([
    addDistTag({ package: 'abc-grand-parent-with-c', version: '1.0.0', distTag: 'latest' }),
    addDistTag({ package: 'abc-parent-with-ab', version: '1.0.0', distTag: 'latest' }),
    addDistTag({ package: 'bar', version: '100.0.0', distTag: 'latest' }),
    addDistTag({ package: 'foo', version: '100.0.0', distTag: 'latest' }),
    addDistTag({ package: 'foobarqar', version: '1.0.0', distTag: 'latest' }),
    addDistTag({ package: 'peer-c', version: '1.0.0', distTag: 'latest' }),
  ])

  const manifest = await addDependenciesToPackage({}, ['foobarqar', 'abc-grand-parent-with-c'], await testDefaults({ lockfileOnly: true }))

  await Promise.all([
    addDistTag({ package: 'abc-grand-parent-with-c', version: '1.0.1', distTag: 'latest' }),
    addDistTag({ package: 'abc-parent-with-ab', version: '1.0.1', distTag: 'latest' }),
    addDistTag({ package: 'bar', version: '100.1.0', distTag: 'latest' }),
    addDistTag({ package: 'foo', version: '100.1.0', distTag: 'latest' }),
    addDistTag({ package: 'foobarqar', version: '1.0.1', distTag: 'latest' }),
  ])

  await install(manifest, await testDefaults({ update: true, depth: 0 }))

  const lockfile = await project.readLockfile()

  expect(lockfile.packages).toBeTruthy()
  expect(lockfile.packages).toHaveProperty(['/abc-parent-with-ab/1.0.0_peer-c@1.0.0']) // preserve version of package that has resolved peer deps
  expect(lockfile.packages).toHaveProperty(['/foobarqar/1.0.1'])
  expect(lockfile.packages['/foobarqar/1.0.1'].dependencies).toStrictEqual({
    bar: '100.0.0',
    foo: '100.0.0',
    qar: '100.0.0',
  })
})

test('update does not fail when package has only peer dependencies', async () => {
  prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['has-pkg-with-peer-only'], await testDefaults())

  await install(manifest, await testDefaults({ update: true, depth: Infinity }))
})

test('update does not install the package if it is not present in package.json', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['is-positive'], await testDefaults({
    allowNew: false,
    update: true,
  }))

  await project.hasNot('is-positive')
})

test('update dependency when external lockfile directory is used', async () => {
  prepareEmpty()

  await addDistTag({ package: 'foo', version: '100.0.0', distTag: 'latest' })

  const lockfileDir = path.resolve('..')
  const manifest = await addDependenciesToPackage({}, ['foo'], await testDefaults({ lockfileDir }))

  await addDistTag({ package: 'foo', version: '100.1.0', distTag: 'latest' })

  await install(manifest, await testDefaults({ update: true, depth: 0, lockfileDir }))

  const lockfile = await readYamlFile<Lockfile>(path.join('..', WANTED_LOCKFILE))

  expect(lockfile.packages).toHaveProperty(['/foo/100.1.0'])
})

// Covers https://github.com/pnpm/pnpm/issues/2191
test('preserve subdeps when installing on a package that has one dependency spec changed in the manifest', async () => {
  const project = prepareEmpty()

  await Promise.all([
    addDistTag({ package: 'abc-grand-parent-with-c', version: '1.0.0', distTag: 'latest' }),
    addDistTag({ package: 'abc-parent-with-ab', version: '1.0.0', distTag: 'latest' }),
    addDistTag({ package: 'bar', version: '100.0.0', distTag: 'latest' }),
    addDistTag({ package: 'foo', version: '100.0.0', distTag: 'latest' }),
    addDistTag({ package: 'foobarqar', version: '1.0.0', distTag: 'latest' }),
    addDistTag({ package: 'peer-c', version: '1.0.0', distTag: 'latest' }),
  ])

  const manifest = await addDependenciesToPackage({}, ['foobarqar', 'abc-grand-parent-with-c'], await testDefaults())

  manifest.dependencies!['foobarqar'] = '^1.0.1'

  await Promise.all([
    addDistTag({ package: 'abc-parent-with-ab', version: '1.0.1', distTag: 'latest' }),
    addDistTag({ package: 'bar', version: '100.1.0', distTag: 'latest' }),
    addDistTag({ package: 'foo', version: '100.1.0', distTag: 'latest' }),
    addDistTag({ package: 'foobarqar', version: '1.0.1', distTag: 'latest' }),
  ])

  await install(manifest, await testDefaults())

  const lockfile = await project.readLockfile()

  expect(lockfile.packages).toHaveProperty(['/abc-parent-with-ab/1.0.0_peer-c@1.0.0']) // preserve version of package that has resolved peer deps
  expect(lockfile.packages).toHaveProperty(['/foobarqar/1.0.1'])
  expect(lockfile.packages['/foobarqar/1.0.1'].dependencies).toStrictEqual({
    bar: '100.0.0',
    foo: '100.0.0',
    qar: '100.0.0',
  })
})

// Covers https://github.com/pnpm/pnpm/issues/2226
test('update only the packages that were requested to be updated when hoisting is on', async () => {
  const project = prepareEmpty()

  await addDistTag({ package: 'bar', version: '100.0.0', distTag: 'latest' })
  await addDistTag({ package: 'foo', version: '100.0.0', distTag: 'latest' })

  let manifest = await addDependenciesToPackage({}, ['bar', 'foo'], await testDefaults({ hoistPattern: ['*'] }))

  await addDistTag({ package: 'bar', version: '100.1.0', distTag: 'latest' })
  await addDistTag({ package: 'foo', version: '100.1.0', distTag: 'latest' })

  manifest = await addDependenciesToPackage(manifest, ['foo'], await testDefaults({ allowNew: false, update: true, hoistPattern: ['*'] }))

  expect(manifest.dependencies).toStrictEqual({ bar: '^100.0.0', foo: '^100.1.0' })

  const lockfile = await project.readLockfile()
  expect(Object.keys(lockfile.packages)).toStrictEqual(['/bar/100.0.0', '/foo/100.1.0'])
})

test('update only the specified package', async () => {
  const project = prepareEmpty()

  await Promise.all([
    addDistTag({ package: 'abc-grand-parent-with-c', version: '1.0.0', distTag: 'latest' }),
    addDistTag({ package: 'abc-parent-with-ab', version: '1.0.0', distTag: 'latest' }),
    addDistTag({ package: 'bar', version: '100.0.0', distTag: 'latest' }),
    addDistTag({ package: 'foo', version: '100.0.0', distTag: 'latest' }),
    addDistTag({ package: 'foobarqar', version: '1.0.0', distTag: 'latest' }),
    addDistTag({ package: 'peer-c', version: '1.0.0', distTag: 'latest' }),
  ])

  const manifest = await addDependenciesToPackage({}, ['foobarqar', 'abc-grand-parent-with-c'], await testDefaults())

  await Promise.all([
    addDistTag({ package: 'abc-grand-parent-with-c', version: '1.0.1', distTag: 'latest' }),
    addDistTag({ package: 'abc-parent-with-ab', version: '1.0.1', distTag: 'latest' }),
    addDistTag({ package: 'bar', version: '100.1.0', distTag: 'latest' }),
    addDistTag({ package: 'foo', version: '100.1.0', distTag: 'latest' }),
    addDistTag({ package: 'foobarqar', version: '1.0.1', distTag: 'latest' }),
  ])

  await install(manifest, await testDefaults({
    depth: Infinity,
    update: true,
    updateMatching: (pkgName: string) => pkgName === 'foo',
  }))

  const lockfile = await project.readLockfile()

  expect(lockfile.packages).toHaveProperty(['/abc-parent-with-ab/1.0.0_peer-c@1.0.0'])
  expect(lockfile.packages).toHaveProperty(['/foobarqar/1.0.0'])
  expect(lockfile.packages['/foobarqar/1.0.0'].dependencies).toStrictEqual({
    bar: '100.0.0',
    foo: '100.1.0',
    'is-positive': '3.1.0',
  })
})
