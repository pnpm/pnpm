import tape = require('tape')
import promisifyTape from 'tape-promise'
import rimraf = require('rimraf-then')
import {prepare, testDefaults, execPnpm} from './utils'

const test = promisifyTape(tape)

test('CLI fails when store status finds modified packages', async function (t: tape.Test) {
  const project = prepare(t)

  await execPnpm('install', 'is-positive@3.1.0')

  const isPositive = await project.resolve('is-positive', '3.1.0', 'index.js')
  await rimraf(isPositive)

  try {
    await execPnpm('store', 'status')
    t.fail('CLI should have failed')
  } catch (err) {
    t.pass('CLI failed')
  }
})

test('CLI does not fail when store status does not find modified packages', async function (t: tape.Test) {
  const project = prepare(t)

  await execPnpm('install', 'is-positive@3.1.0')

  await execPnpm('store', 'status')
  t.pass('CLI did not fail')
})
