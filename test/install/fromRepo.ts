import tape = require('tape')
import promisifyTape from 'tape-promise'
import isCI = require('is-ci')
import readPkg = require('read-pkg')
import {installPkgs} from '../../src'
import {
  prepare,
  testDefaults,
} from '../utils'

const test = promisifyTape(tape)

test('from a github repo', async function (t) {
  const project = prepare(t)
  await installPkgs(['kevva/is-negative'], testDefaults())

  const localPkg = project.requireModule('is-negative')

  t.ok(localPkg, 'isNegative() is available')

  const pkgJson = await readPkg()
  t.deepEqual(pkgJson.dependencies, {'is-negative': 'github:kevva/is-negative'}, 'has been added to dependencies in package.json')
})

test('from a git repo', async function (t) {
  if (isCI) {
    t.skip('not testing the SSH GIT access via CI')
    return t.end()
  }
  const project = prepare(t)
  await installPkgs(['git+ssh://git@github.com/kevva/is-negative.git'], testDefaults())

  const localPkg = project.requireModule('is-negative')

  t.ok(localPkg, 'isNegative() is available')
})
