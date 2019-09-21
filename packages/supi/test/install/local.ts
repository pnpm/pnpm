import { WANTED_LOCKFILE } from '@pnpm/constants'
import { prepareEmpty } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import rimraf = require('@zkochan/rimraf')
import { copy } from 'fs-extra'
import fs = require('mz/fs')
import ncpCB = require('ncp')
import normalizePath = require('normalize-path')
import path = require('path')
import {
  addDependenciesToPackage,
  install,
  mutateModules,
} from 'supi'
import symlinkDir = require('symlink-dir')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { promisify } from 'util'
import {
  local,
  pathToLocalPkg,
  testDefaults,
} from '../utils'

const ncp = promisify(ncpCB.ncp)
const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('scoped modules from a directory', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  await addDependenciesToPackage({}, [local('local-scoped-pkg')], await testDefaults())

  const m = project.requireModule('@scope/local-scoped-pkg')

  t.equal(m(), '@scope/local-scoped-pkg', 'localScopedPkg() is available')
})

test('local file', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  await ncp(pathToLocalPkg('local-pkg'), path.resolve('..', 'local-pkg'))

  const manifest = await addDependenciesToPackage({}, ['file:../local-pkg'], await testDefaults())

  const expectedSpecs = { 'local-pkg': `link:..${path.sep}local-pkg` }
  t.deepEqual(manifest.dependencies, expectedSpecs, 'local-pkg has been added to dependencies')

  const m = project.requireModule('local-pkg')

  t.ok(m, 'localPkg() is available')

  const lockfile = await project.readLockfile()

  t.deepEqual(lockfile, {
    dependencies: {
      'local-pkg': 'link:../local-pkg',
    },
    lockfileVersion: 5.1,
    specifiers: expectedSpecs,
  })
})

test('local file via link:', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  await ncp(pathToLocalPkg('local-pkg'), path.resolve('..', 'local-pkg'))

  const manifest = await addDependenciesToPackage({}, ['link:../local-pkg'], await testDefaults())

  const expectedSpecs = { 'local-pkg': `link:..${path.sep}local-pkg` }
  t.deepEqual(manifest.dependencies, expectedSpecs, 'local-pkg has been added to dependencies')

  const m = project.requireModule('local-pkg')

  t.ok(m, 'localPkg() is available')

  const lockfile = await project.readLockfile()

  t.deepEqual(lockfile, {
    dependencies: {
      'local-pkg': 'link:../local-pkg',
    },
    lockfileVersion: 5.1,
    specifiers: expectedSpecs,
  })
})

test('local file with symlinked node_modules', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  await ncp(pathToLocalPkg('local-pkg'), path.resolve('..', 'local-pkg'))
  await fs.mkdir(path.join('..', 'node_modules'))
  await symlinkDir(path.join('..', 'node_modules'), 'node_modules')

  const manifest = await addDependenciesToPackage({}, ['file:../local-pkg'], await testDefaults())

  const expectedSpecs = { 'local-pkg': `link:..${path.sep}local-pkg` }
  t.deepEqual(manifest.dependencies, expectedSpecs, 'local-pkg has been added to dependencies')

  const m = project.requireModule('local-pkg')

  t.ok(m, 'localPkg() is available')

  const lockfile = await project.readLockfile()

  t.deepEqual(lockfile, {
    dependencies: {
      'local-pkg': 'link:../local-pkg',
    },
    lockfileVersion: 5.1,
    specifiers: expectedSpecs,
  })
})

test('package with a broken symlink', async (t) => {
  const project = prepareEmpty(t)
  await addDependenciesToPackage({}, [pathToLocalPkg('has-broken-symlink/has-broken-symlink.tar.gz')], await testDefaults())

  const m = project.requireModule('has-broken-symlink')

  t.ok(m, 'has-broken-symlink is available')
})

test('tarball local package', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  const manifest = await addDependenciesToPackage({}, [pathToLocalPkg('tar-pkg/tar-pkg-1.0.0.tgz')], await testDefaults())

  const m = project.requireModule('tar-pkg')

  t.equal(m(), 'tar-pkg', 'tarPkg() is available')

  const pkgSpec = `file:${normalizePath(pathToLocalPkg('tar-pkg/tar-pkg-1.0.0.tgz'))}`
  t.deepEqual(manifest.dependencies, { 'tar-pkg': pkgSpec }, 'has been added to dependencies in package.json')

  const lockfile = await project.readLockfile()
  t.deepEqual(lockfile.packages[lockfile.dependencies['tar-pkg']], {
    dev: false,
    name: 'tar-pkg',
    resolution: {
      integrity: 'sha512-HP/5Rgt3pVFLzjmN9qJJ6vZMgCwoCIl/m2bPndYT283CUqnmFiMx0GeeIJ7SyK6TYoJM78SEvFEOQie++caHqw==',
      tarball: `file:${normalizePath(path.relative(process.cwd(), pathToLocalPkg('tar-pkg/tar-pkg-1.0.0.tgz')))}`,
    },
    version: '1.0.0',
  }, `a snapshot of the local dep tarball added to ${WANTED_LOCKFILE}`)
})

test('tarball local package from project directory', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await copy(path.join(pathToLocalPkg('tar-pkg'), 'tar-pkg-1.0.0.tgz'), path.resolve('tar-pkg-1.0.0.tgz'))

  const manifest = await install({
    dependencies: {
      'tar-pkg': 'file:tar-pkg-1.0.0.tgz',
    },
  }, await testDefaults())

  const m = project.requireModule('tar-pkg')

  t.equal(m(), 'tar-pkg', 'tarPkg() is available')

  const pkgSpec = `file:tar-pkg-1.0.0.tgz`
  t.deepEqual(manifest.dependencies, { 'tar-pkg': pkgSpec }, 'has been added to dependencies in package.json')

  const lockfile = await project.readLockfile()
  t.equal(lockfile.dependencies['tar-pkg'], pkgSpec)
  t.deepEqual(lockfile.packages[lockfile.dependencies['tar-pkg']], {
    dev: false,
    name: 'tar-pkg',
    resolution: {
      integrity: 'sha512-HP/5Rgt3pVFLzjmN9qJJ6vZMgCwoCIl/m2bPndYT283CUqnmFiMx0GeeIJ7SyK6TYoJM78SEvFEOQie++caHqw==',
      tarball: pkgSpec,
    },
    version: '1.0.0',
  }, `a snapshot of the local dep tarball added to ${WANTED_LOCKFILE}`)
})

test('update tarball local package when its integrity changes', async (t) => {
  const project = prepareEmpty(t)

  await ncp(pathToLocalPkg('tar-pkg-with-dep-1/tar-pkg-with-dep-1.0.0.tgz'), path.resolve('..', 'tar.tgz'))
  const manifest = await addDependenciesToPackage({}, ['../tar.tgz'], await testDefaults())

  const lockfile1 = await project.readLockfile()
  t.equal(lockfile1.packages['file:../tar.tgz'].dependencies!['is-positive'], '1.0.0')

  await ncp(pathToLocalPkg('tar-pkg-with-dep-2/tar-pkg-with-dep-1.0.0.tgz'), path.resolve('..', 'tar.tgz'))
  await install(manifest, await testDefaults())

  const lockfile2 = await project.readLockfile()
  t.equal(lockfile2.packages['file:../tar.tgz'].dependencies!['is-positive'], '2.0.0', 'the local tarball dep has been updated')
})

// Covers https://github.com/pnpm/pnpm/issues/1878
test('do not update deps when installing in a project that has local tarball dep', async (t) => {
  await addDistTag({ package: 'peer-a', version: '1.0.0', distTag: 'latest' })
  const project = prepareEmpty(t)

  await ncp(pathToLocalPkg('tar-pkg-with-dep-1/tar-pkg-with-dep-1.0.0.tgz'), path.resolve('..', 'tar.tgz'))
  const manifest = await addDependenciesToPackage({}, ['../tar.tgz', 'peer-a'], await testDefaults({ lockfileOnly: true }))

  const initialLockfile = await project.readLockfile()

  await addDistTag({ package: 'peer-a', version: '1.0.1', distTag: 'latest' })

  await mutateModules([
    {
      buildIndex: 0,
      manifest,
      mutation: 'install',
      prefix: process.cwd(),
    }
  ], await testDefaults())

  const latestLockfile = await project.readLockfile()

  t.deepEqual(initialLockfile, latestLockfile)
})

// Covers https://github.com/pnpm/pnpm/issues/1882
test(`frozen-lockfile: installation fails if the integrity of a tarball dependency changed`, async (t) => {
  prepareEmpty(t)

  await ncp(pathToLocalPkg('tar-pkg-with-dep-1/tar-pkg-with-dep-1.0.0.tgz'), path.resolve('..', 'tar.tgz'))
  const manifest = await addDependenciesToPackage({}, ['../tar.tgz'], await testDefaults())

  await rimraf('node_modules')

  await ncp(pathToLocalPkg('tar-pkg-with-dep-2/tar-pkg-with-dep-1.0.0.tgz'), path.resolve('..', 'tar.tgz'))

  let err!: Error
  try {
    await install(manifest, await testDefaults({ frozenLockfile: true }))
  } catch (_err) {
    err = _err
  }

  t.ok(err)
  t.equal(err['code'], 'EINTEGRITY')
})
