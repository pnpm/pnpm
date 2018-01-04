import tape = require('tape')
import promisifyTape from 'tape-promise'
import normalizePath = require('normalize-path')
import readPkg = require('read-pkg')
import ncpCB = require('ncp')
import thenify = require('thenify')
import path = require('path')
import {install, installPkgs} from 'supi'
import {
  prepare,
  testDefaults,
  pathToLocalPkg,
  local,
} from '../utils'

const ncp = thenify(ncpCB.ncp)
const test = promisifyTape(tape)

test('scoped modules from a directory', async function (t: tape.Test) {
  const project = prepare(t)
  await installPkgs([local('local-scoped-pkg')], await testDefaults())

  const m = project.requireModule('@scope/local-scoped-pkg')

  t.equal(m(), '@scope/local-scoped-pkg', 'localScopedPkg() is available')
})

test('local file', async function (t: tape.Test) {
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
    specifiers: expectedSpecs,
    dependencies: {
      'local-pkg': 'file:../local-pkg',
    },
    registry: 'http://localhost:4873/',
    shrinkwrapVersion: 3,
    shrinkwrapMinorVersion: 4,
  })
})

test('package with a broken symlink', async function (t) {
  const project = prepare(t)
  await installPkgs([pathToLocalPkg('has-broken-symlink/has-broken-symlink.tar.gz')], await testDefaults())

  const m = project.requireModule('has-broken-symlink')

  t.ok(m, 'has-broken-symlink is available')
})

test('tarball local package', async function (t) {
  const project = prepare(t)
  await installPkgs([pathToLocalPkg('tar-pkg/tar-pkg-1.0.0.tgz')], await testDefaults())

  const m = project.requireModule('tar-pkg')

  t.equal(m(), 'tar-pkg', 'tarPkg() is available')

  const pkgJson = await readPkg()
  t.deepEqual(pkgJson.dependencies,
    {'tar-pkg': `file:${normalizePath(pathToLocalPkg('tar-pkg/tar-pkg-1.0.0.tgz'))}`},
    'has been added to dependencies in package.json')
})
