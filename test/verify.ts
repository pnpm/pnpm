import tape = require('tape')
import promisifyTape from 'tape-promise'
import rimraf = require('rimraf-then')
import {prepare, testDefaults, execPnpm} from './utils'
import {verify, installPkgs} from '../src'

const test = promisifyTape(tape)

test('verify returns empty array when store was not modified', async function (t: tape.Test) {
  const project = prepare(t)

  const opts = testDefaults()
  await installPkgs(['is-positive@3.1.0'], opts)

  const mutatedPkgs = await verify(opts)

  t.equal(mutatedPkgs && mutatedPkgs.length, 0, 'no packages were modified')
})

test('verify returns path to the modified package', async function (t: tape.Test) {
  const project = prepare(t)

  const opts = testDefaults()
  await installPkgs(['is-positive@3.1.0'], opts)

  const isPositive = await project.resolve('is-positive', '3.1.0', 'index.js')
  await rimraf(isPositive)

  const mutatedPkgs = await verify(opts)

  t.equal(mutatedPkgs && mutatedPkgs.length, 1, '1 package was modified')
  t.ok(mutatedPkgs && mutatedPkgs[0].indexOf('is-positive') !== -1, 'is-positive was modified')
})

test('CLI fails when verify finds modified packages', async function (t: tape.Test) {
  const project = prepare(t)

  const opts = testDefaults()
  await installPkgs(['is-positive@3.1.0'], opts)

  const isPositive = await project.resolve('is-positive', '3.1.0', 'index.js')
  await rimraf(isPositive)

  try {
    await execPnpm('verify')
    t.fail('CLI should have failed')
  } catch (err) {
    t.pass('CLI failed')
  }
})

test('CLI does not fail when verify does not find modified packages', async function (t: tape.Test) {
  const project = prepare(t)

  const opts = testDefaults()
  await installPkgs(['is-positive@3.1.0'], opts)

  await execPnpm('verify')
  t.pass('CLI did not fail')
})
