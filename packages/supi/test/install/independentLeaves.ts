import path = require('path')
import {installPkgs} from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import {
  prepare,
  testDefaults,
} from '../utils'

const test = promisifyTape(tape)

test('install with --independent-leaves', async (t: tape.Test) => {
  const project = prepare(t)
  await installPkgs(['rimraf@2.5.1'], await testDefaults({independentLeaves: true}))

  const m = project.requireModule('rimraf')
  t.ok(typeof m === 'function', 'rimraf() is available')
  await project.isExecutable('.bin/rimraf')
})

test('--independent-leaves throws exception when executed on node_modules installed w/o the option', async (t: tape.Test) => {
  const project = prepare(t)
  await installPkgs(['is-positive'], await testDefaults({independentLeaves: false}))

  try {
    await installPkgs(['is-negative'], await testDefaults({independentLeaves: true}))
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err['code'], 'ERR_PNPM_INDEPENDENT_LEAVES_NOT_WANTED') // tslint:disable-line:no-string-literal
    t.ok(err.message.indexOf('This node_modules was not installed with the --independent-leaves option.') === 0)
  }
})

test('--no-independent-leaves throws exception when executed on node_modules installed with --independent-leaves', async (t: tape.Test) => {
  const project = prepare(t)
  await installPkgs(['is-positive'], await testDefaults({independentLeaves: true}))

  try {
    await installPkgs(['is-negative'], await testDefaults({independentLeaves: false}))
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err['code'], 'ERR_PNPM_INDEPENDENT_LEAVES_WANTED') // tslint:disable-line:no-string-literal
    t.ok(err.message.indexOf('This node_modules was installed with --independent-leaves option.') === 0)
  }
})
