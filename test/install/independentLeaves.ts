import tape = require('tape')
import path = require('path')
import promisifyTape from 'tape-promise'
import {
  prepare,
  testDefaults,
} from '../utils'
import {installPkgs} from 'supi'

const test = promisifyTape(tape)

test('install with --independent-leaves', async function (t: tape.Test) {
  const project = prepare(t)
  await installPkgs(['rimraf@2.5.1'], await testDefaults({independentLeaves: true}))

  const m = project.requireModule('rimraf')
  t.ok(typeof m === 'function', 'rimraf() is available')
  await project.isExecutable('.bin/rimraf')
})

test('--independent-leaves throws exception when executed on node_modules installed w/o the option', async function (t: tape.Test) {
  const project = prepare(t)
  await installPkgs(['is-positive'], await testDefaults({independentLeaves: false}))

  try {
    await installPkgs(['is-negative'], await testDefaults({independentLeaves: true}))
    t.fail('installation should have failed')
  } catch (err) {
    t.ok(err.message.indexOf('This node_modules was not installed with the --independent-leaves option.') === 0)
  }
})

test('--no-independent-leaves throws exception when executed on node_modules installed with --independent-leaves', async function (t: tape.Test) {
  const project = prepare(t)
  await installPkgs(['is-positive'], await testDefaults({independentLeaves: true}))

  try {
    await installPkgs(['is-negative'], await testDefaults({independentLeaves: false}))
    t.fail('installation should have failed')
  } catch (err) {
    t.ok(err.message.indexOf('This node_modules was installed with --independent-leaves option.') === 0)
  }
})

test('global installation with --independent-leaves', async function (t: tape.Test) {
  prepare(t)
  const globalPrefix = path.resolve('..', 'global')
  const opts = await testDefaults({global: true, prefix: globalPrefix, independentLeaves: true})
  await installPkgs(['is-positive'], opts)

  // there was an issue when subsequent installations were removing everything installed prior
  // https://github.com/pnpm/pnpm/issues/808
  await installPkgs(['is-negative'], opts)

  const isPositive = require(path.join(globalPrefix, '1_independent_leaves', 'node_modules', 'is-positive'))
  t.ok(typeof isPositive === 'function', 'isPositive() is available')

  const isNegative = require(path.join(globalPrefix, '1_independent_leaves', 'node_modules', 'is-negative'))
  t.ok(typeof isNegative === 'function', 'isNegative() is available')
})
