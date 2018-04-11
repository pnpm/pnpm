import fs = require('mz/fs')
import ncpCB = require('ncp')
import normalizePath = require('normalize-path')
import path = require('path')
import readPkg = require('read-pkg')
import {install, installPkgs} from 'supi'
import symlinkDir = require('symlink-dir')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import promisify = require('util.promisify')
import {
  local,
  pathToLocalPkg,
  prepare,
  testDefaults,
} from '../utils'

const ncp = promisify(ncpCB.ncp)
const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('scoped modules from a directory', async (t: tape.Test) => {
  const project = prepare(t)
  await installPkgs([local('local-scoped-pkg')], await testDefaults())

  const m = project.requireModule('@scope/local-scoped-pkg')

  t.equal(m(), '@scope/local-scoped-pkg', 'localScopedPkg() is available')
})

test('local file', async (t: tape.Test) => {
  const project = prepare(t)
  await ncp(pathToLocalPkg('local-pkg'), path.resolve('..', 'local-pkg'))

  await installPkgs(['file:../local-pkg'], await testDefaults())

  const pkgJson = await readPkg()
  const expectedSpecs = {'local-pkg': `file:..${path.sep}local-pkg`}
  t.deepEqual(pkgJson.dependencies, expectedSpecs, 'local-pkg has been added to dependencies')

  const m = project.requireModule('local-pkg')

  t.ok(m, 'localPkg() is available')

  const shr = await project.loadShrinkwrap()

  t.deepEqual(shr, {
    dependencies: {
      'local-pkg': 'file:../local-pkg',
    },
    registry: 'http://localhost:4873/',
    shrinkwrapMinorVersion: 5,
    shrinkwrapVersion: 3,
    specifiers: expectedSpecs,
  })
})

test('local file via link:', async (t: tape.Test) => {
  const project = prepare(t)
  await ncp(pathToLocalPkg('local-pkg'), path.resolve('..', 'local-pkg'))

  await installPkgs(['link:../local-pkg'], await testDefaults())

  const pkgJson = await readPkg()
  const expectedSpecs = {'local-pkg': `link:..${path.sep}local-pkg`}
  t.deepEqual(pkgJson.dependencies, expectedSpecs, 'local-pkg has been added to dependencies')

  const m = project.requireModule('local-pkg')

  t.ok(m, 'localPkg() is available')

  const shr = await project.loadShrinkwrap()

  t.deepEqual(shr, {
    dependencies: {
      'local-pkg': 'link:../local-pkg',
    },
    registry: 'http://localhost:4873/',
    shrinkwrapMinorVersion: 5,
    shrinkwrapVersion: 3,
    specifiers: expectedSpecs,
  })
})

test('local file with symlinked node_modules', async (t: tape.Test) => {
  const project = prepare(t)
  await ncp(pathToLocalPkg('local-pkg'), path.resolve('..', 'local-pkg'))
  await fs.mkdir(path.join('..', 'node_modules'))
  await symlinkDir(path.join('..', 'node_modules'), 'node_modules')

  await installPkgs(['file:../local-pkg'], await testDefaults())

  const pkgJson = await readPkg()
  const expectedSpecs = {'local-pkg': `file:..${path.sep}local-pkg`}
  t.deepEqual(pkgJson.dependencies, expectedSpecs, 'local-pkg has been added to dependencies')

  const m = project.requireModule('local-pkg')

  t.ok(m, 'localPkg() is available')

  const shr = await project.loadShrinkwrap()

  t.deepEqual(shr, {
    dependencies: {
      'local-pkg': 'file:../local-pkg',
    },
    registry: 'http://localhost:4873/',
    shrinkwrapMinorVersion: 5,
    shrinkwrapVersion: 3,
    specifiers: expectedSpecs,
  })
})

test('package with a broken symlink', async (t) => {
  const project = prepare(t)
  await installPkgs([pathToLocalPkg('has-broken-symlink/has-broken-symlink.tar.gz')], await testDefaults())

  const m = project.requireModule('has-broken-symlink')

  t.ok(m, 'has-broken-symlink is available')
})

test('tarball local package', async (t) => {
  const project = prepare(t)
  await installPkgs([pathToLocalPkg('tar-pkg/tar-pkg-1.0.0.tgz')], await testDefaults())

  const m = project.requireModule('tar-pkg')

  t.equal(m(), 'tar-pkg', 'tarPkg() is available')

  const pkgJson = await readPkg()
  const pkgSpec = `file:${normalizePath(pathToLocalPkg('tar-pkg/tar-pkg-1.0.0.tgz'))}`
  t.deepEqual(pkgJson.dependencies, {'tar-pkg': pkgSpec}, 'has been added to dependencies in package.json')

  const shr = await project.loadShrinkwrap()
  t.deepEqual(shr.packages[shr.dependencies['tar-pkg']], {
    dev: false,
    name: 'tar-pkg',
    resolution: {
      integrity: 'sha512-HP/5Rgt3pVFLzjmN9qJJ6vZMgCwoCIl/m2bPndYT283CUqnmFiMx0GeeIJ7SyK6TYoJM78SEvFEOQie++caHqw==',
      tarball: 'file:../../../supi/test/packages/tar-pkg/tar-pkg-1.0.0.tgz',
    },
    version: '1.0.0',
  }, 'a snapshot of the local dep tarball added to shrinkwrap.yaml')
})

test('update tarball local package when its integrity changes', async (t) => {
  const project = prepare(t)

  await ncp(pathToLocalPkg('tar-pkg-with-dep-1/tar-pkg-with-dep-1.0.0.tgz'), path.resolve('..', 'tar.tgz'))
  await installPkgs(['../tar.tgz'], await testDefaults())

  const shr1 = await project.loadShrinkwrap()
  t.equal(shr1.packages['file:../tar.tgz'].dependencies['is-positive'], '1.0.0')

  await ncp(pathToLocalPkg('tar-pkg-with-dep-2/tar-pkg-with-dep-1.0.0.tgz'), path.resolve('..', 'tar.tgz'))
  await install(await testDefaults())

  const shr2 = await project.loadShrinkwrap()
  t.equal(shr2.packages['file:../tar.tgz'].dependencies['is-positive'], '2.0.0', 'the local tarball dep has been updated')
})
