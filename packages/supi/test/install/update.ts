import { WANTED_LOCKFILE } from '@pnpm/constants'
import { Lockfile } from '@pnpm/lockfile-file'
import { prepareEmpty } from '@pnpm/prepare'
import readYamlFile from 'read-yaml-file'
import { addDependenciesToPackage, install } from 'supi'
import promisifyTape from 'tape-promise'
import {
  addDistTag,
  testDefaults,
} from '../utils'
import path = require('path')
import tape = require('tape')

const test = promisifyTape(tape)

test('preserve subdeps on update', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await Promise.all([
    addDistTag('abc-grand-parent-with-c', '1.0.0', 'latest'),
    addDistTag('abc-parent-with-ab', '1.0.0', 'latest'),
    addDistTag('bar', '100.0.0', 'latest'),
    addDistTag('foo', '100.0.0', 'latest'),
    addDistTag('foobarqar', '1.0.0', 'latest'),
    addDistTag('peer-c', '1.0.0', 'latest'),
  ])

  const manifest = await addDependenciesToPackage({}, ['foobarqar', 'abc-grand-parent-with-c'], await testDefaults())

  await Promise.all([
    addDistTag('abc-grand-parent-with-c', '1.0.1', 'latest'),
    addDistTag('abc-parent-with-ab', '1.0.1', 'latest'),
    addDistTag('bar', '100.1.0', 'latest'),
    addDistTag('foo', '100.1.0', 'latest'),
    addDistTag('foobarqar', '1.0.1', 'latest'),
  ])

  await install(manifest, await testDefaults({ update: true, depth: 0 }))

  const lockfile = await project.readLockfile()

  t.ok(lockfile.packages)
  t.ok(lockfile.packages['/abc-parent-with-ab/1.0.0_peer-c@1.0.0'], 'preserve version of package that has resolved peer deps')
  t.ok(lockfile.packages['/foobarqar/1.0.1'])
  t.deepEqual(lockfile.packages['/foobarqar/1.0.1'].dependencies, {
    bar: '100.0.0',
    foo: '100.0.0',
    qar: '100.0.0',
  })
})

test('preserve subdeps on update when no node_modules is present', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await Promise.all([
    addDistTag('abc-grand-parent-with-c', '1.0.0', 'latest'),
    addDistTag('abc-parent-with-ab', '1.0.0', 'latest'),
    addDistTag('bar', '100.0.0', 'latest'),
    addDistTag('foo', '100.0.0', 'latest'),
    addDistTag('foobarqar', '1.0.0', 'latest'),
    addDistTag('peer-c', '1.0.0', 'latest'),
  ])

  const manifest = await addDependenciesToPackage({}, ['foobarqar', 'abc-grand-parent-with-c'], await testDefaults({ lockfileOnly: true }))

  await Promise.all([
    addDistTag('abc-grand-parent-with-c', '1.0.1', 'latest'),
    addDistTag('abc-parent-with-ab', '1.0.1', 'latest'),
    addDistTag('bar', '100.1.0', 'latest'),
    addDistTag('foo', '100.1.0', 'latest'),
    addDistTag('foobarqar', '1.0.1', 'latest'),
  ])

  await install(manifest, await testDefaults({ update: true, depth: 0 }))

  const lockfile = await project.readLockfile()

  t.ok(lockfile.packages)
  t.ok(lockfile.packages['/abc-parent-with-ab/1.0.0_peer-c@1.0.0'], 'preserve version of package that has resolved peer deps')
  t.ok(lockfile.packages['/foobarqar/1.0.1'])
  t.deepEqual(lockfile.packages['/foobarqar/1.0.1'].dependencies, {
    bar: '100.0.0',
    foo: '100.0.0',
    qar: '100.0.0',
  })
})

test('update does not fail when package has only peer dependencies', async (t: tape.Test) => {
  prepareEmpty(t)

  const manifest = await addDependenciesToPackage({}, ['has-pkg-with-peer-only'], await testDefaults())

  await install(manifest, await testDefaults({ update: true, depth: Infinity }))

  t.pass('did not fail')
})

test('update does not install the package if it is not present in package.json', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDependenciesToPackage({}, ['is-positive'], await testDefaults({
    allowNew: false,
    update: true,
  }))

  await project.hasNot('is-positive')
})

test('update dependency when external lockfile directory is used', async (t: tape.Test) => {
  prepareEmpty(t)

  await addDistTag('foo', '100.0.0', 'latest')

  const lockfileDir = path.resolve('..')
  const manifest = await addDependenciesToPackage({}, ['foo'], await testDefaults({ lockfileDir }))

  await addDistTag('foo', '100.1.0', 'latest')

  await install(manifest, await testDefaults({ update: true, depth: 0, lockfileDir }))

  const lockfile = await readYamlFile<Lockfile>(path.join('..', WANTED_LOCKFILE))

  t.ok(lockfile.packages?.['/foo/100.1.0'])
})

// Covers https://github.com/pnpm/pnpm/issues/2191
test('preserve subdeps when installing on a package that has one dependency spec changed in the manifest', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await Promise.all([
    addDistTag('abc-grand-parent-with-c', '1.0.0', 'latest'),
    addDistTag('abc-parent-with-ab', '1.0.0', 'latest'),
    addDistTag('bar', '100.0.0', 'latest'),
    addDistTag('foo', '100.0.0', 'latest'),
    addDistTag('foobarqar', '1.0.0', 'latest'),
    addDistTag('peer-c', '1.0.0', 'latest'),
  ])

  const manifest = await addDependenciesToPackage({}, ['foobarqar', 'abc-grand-parent-with-c'], await testDefaults())

  manifest.dependencies!['foobarqar'] = '^1.0.1'

  await Promise.all([
    addDistTag('abc-parent-with-ab', '1.0.1', 'latest'),
    addDistTag('bar', '100.1.0', 'latest'),
    addDistTag('foo', '100.1.0', 'latest'),
    addDistTag('foobarqar', '1.0.1', 'latest'),
  ])

  await install(manifest, await testDefaults())

  const lockfile = await project.readLockfile()

  t.ok(lockfile.packages)
  t.ok(lockfile.packages['/abc-parent-with-ab/1.0.0_peer-c@1.0.0'], 'preserve version of package that has resolved peer deps')
  t.ok(lockfile.packages['/foobarqar/1.0.1'])
  t.deepEqual(lockfile.packages['/foobarqar/1.0.1'].dependencies, {
    bar: '100.0.0',
    foo: '100.0.0',
    qar: '100.0.0',
  })
})

// Covers https://github.com/pnpm/pnpm/issues/2226
test('update only the packages that were requested to be updated when hoisting is on', async (t) => {
  const project = prepareEmpty(t)

  await addDistTag('bar', '100.0.0', 'latest')
  await addDistTag('foo', '100.0.0', 'latest')

  let manifest = await addDependenciesToPackage({}, ['bar', 'foo'], await testDefaults({ hoistPattern: ['*'] }))

  await addDistTag('bar', '100.1.0', 'latest')
  await addDistTag('foo', '100.1.0', 'latest')

  manifest = await addDependenciesToPackage(manifest, ['foo'], await testDefaults({ allowNew: false, update: true, hoistPattern: ['*'] }))

  t.deepEqual(manifest.dependencies, { bar: '^100.0.0', foo: '^100.1.0' })

  const lockfile = await project.readLockfile()
  t.deepEqual(Object.keys(lockfile.packages), ['/bar/100.0.0', '/foo/100.1.0'])
})

test('update only the specified package', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await Promise.all([
    addDistTag('abc-grand-parent-with-c', '1.0.0', 'latest'),
    addDistTag('abc-parent-with-ab', '1.0.0', 'latest'),
    addDistTag('bar', '100.0.0', 'latest'),
    addDistTag('foo', '100.0.0', 'latest'),
    addDistTag('foobarqar', '1.0.0', 'latest'),
    addDistTag('peer-c', '1.0.0', 'latest'),
  ])

  const manifest = await addDependenciesToPackage({}, ['foobarqar', 'abc-grand-parent-with-c'], await testDefaults())

  await Promise.all([
    addDistTag('abc-grand-parent-with-c', '1.0.1', 'latest'),
    addDistTag('abc-parent-with-ab', '1.0.1', 'latest'),
    addDistTag('bar', '100.1.0', 'latest'),
    addDistTag('foo', '100.1.0', 'latest'),
    addDistTag('foobarqar', '1.0.1', 'latest'),
  ])

  await install(manifest, await testDefaults({
    depth: Infinity,
    update: true,
    updateMatching: (pkgName: string) => pkgName === 'foo',
  }))

  const lockfile = await project.readLockfile()

  t.ok(lockfile.packages)
  t.ok(lockfile.packages['/abc-parent-with-ab/1.0.0_peer-c@1.0.0'], 'preserve version of package that has resolved peer deps')
  t.ok(lockfile.packages['/foobarqar/1.0.0'])
  t.deepEqual(lockfile.packages['/foobarqar/1.0.0'].dependencies, {
    bar: '100.0.0',
    foo: '100.1.0',
    'is-positive': '3.1.0',
  })
})
