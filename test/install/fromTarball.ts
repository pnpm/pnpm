import tape = require('tape')
import promisifyTape from 'tape-promise'
import readPkg = require('read-pkg')
import {installPkgs} from 'supi'
import {
  prepare,
  testDefaults,
} from '../utils'

const test = promisifyTape(tape)

test('tarball from npm registry', async function (t) {
  const project = prepare(t)
  await installPkgs(['http://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz'], await testDefaults())

  const m = project.requireModule('is-array')

  t.ok(m, 'isArray() is available')

  await project.storeHas('registry.npmjs.org/is-array/1.0.1')

  const pkgJson = await readPkg()
  t.deepEqual(pkgJson.dependencies, {'is-array': 'http://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz'}, 'has been added to dependencies in package.json')
})

test('tarball not from npm registry', async function (t) {
  const project = prepare(t)
  await installPkgs(['https://github.com/hegemonic/taffydb/tarball/master'], await testDefaults())

  const m = project.requireModule('taffydb')

  t.ok(m, 'taffydb() is available')

  await project.storeHas('github.com/hegemonic/taffydb/tarball/master')
})

test('tarballs from GitHub (is-negative)', async function (t) {
  const project = prepare(t)
  await installPkgs(['is-negative@https://github.com/kevva/is-negative/archive/1d7e288222b53a0cab90a331f1865220ec29560c.tar.gz'], await testDefaults())

  const m = project.requireModule('is-negative')

  t.ok(m, 'isNegative() is available')
})
