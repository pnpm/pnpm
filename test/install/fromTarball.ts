import tape = require('tape')
import promisifyTape from 'tape-promise'
import {installPkgs} from '../../src'
import {
  prepare,
  testDefaults,
} from '../utils'

const test = promisifyTape(tape)

test('tarball from npm registry', async function (t) {
  const project = prepare(t)
  await installPkgs(['http://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz'], testDefaults())

  const isArray = project.requireModule('is-array')

  t.ok(isArray, 'isArray() is available')

  await project.storeHas('registry.npmjs.org/is-array/1.0.1')
})

test('tarball not from npm registry', async function (t) {
  const project = prepare(t)
  await installPkgs(['https://github.com/hegemonic/taffydb/tarball/master'], testDefaults())

  const taffydb = project.requireModule('taffydb')

  t.ok(taffydb, 'taffydb() is available')

  await project.storeHas('github.com/hegemonic/taffydb/tarball/master')
})

test('tarballs from GitHub (is-negative)', async function (t) {
  const project = prepare(t)
  await installPkgs(['is-negative@https://github.com/kevva/is-negative/archive/1d7e288222b53a0cab90a331f1865220ec29560c.tar.gz'], testDefaults())

  const isNegative = project.requireModule('is-negative')

  t.ok(isNegative, 'isNegative() is available')
})
