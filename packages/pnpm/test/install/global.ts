import tape = require('tape')
import promisifyTape from 'tape-promise'
import path = require('path')
import {
  prepare,
  addDistTag,
  execPnpm,
} from '../utils'
import isWindows = require('is-windows')

const test = promisifyTape(tape)
const LAYOUT_VERSION = '1'

test('global installation', async (t: tape.Test) => {
  prepare(t)
  const global = path.resolve('..', 'global')

  if (process.env.APPDATA) process.env.APPDATA = global
  process.env.NPM_CONFIG_PREFIX = global

  await execPnpm('install', '--global', 'is-positive')

  // there was an issue when subsequent installations were removing everything installed prior
  // https://github.com/pnpm/pnpm/issues/808
  await execPnpm('install', '--global', 'is-negative')

  const globalPrefix = isWindows()
    ? path.join(global, 'npm', 'pnpm-global', LAYOUT_VERSION)
    : path.join(global, 'pnpm-global', LAYOUT_VERSION)

  const isPositive = require(path.join(globalPrefix, 'node_modules', 'is-positive'))
  t.ok(typeof isPositive === 'function', 'isPositive() is available')

  const isNegative = require(path.join(globalPrefix, 'node_modules', 'is-negative'))
  t.ok(typeof isNegative === 'function', 'isNegative() is available')
})

test('always install latest when doing global installation without spec', async (t: tape.Test) => {
  prepare(t)
  await addDistTag('peer-c', '2.0.0', 'latest')

  const global = path.resolve('..', 'global')

  if (process.env.APPDATA) process.env.APPDATA = global
  process.env.NPM_CONFIG_PREFIX = global

  await execPnpm('install', '-g', 'peer-c@1')
  await execPnpm('install', '-g', 'peer-c')

  const globalPrefix = isWindows()
    ? path.join(global, 'npm', 'pnpm-global', LAYOUT_VERSION)
    : path.join(global, 'pnpm-global', LAYOUT_VERSION)

  process.chdir(globalPrefix)

  t.equal(require(path.resolve('node_modules', 'peer-c', 'package.json')).version, '2.0.0')
})

test('global installation with --independent-leaves', async (t: tape.Test) => {
  prepare(t)
  const global = path.resolve('..', 'global')

  if (process.env.APPDATA) process.env.APPDATA = global
  process.env.NPM_CONFIG_PREFIX = global

  await execPnpm('install', '-g', '--independent-leaves', 'is-positive')

  // there was an issue when subsequent installations were removing everything installed prior
  // https://github.com/pnpm/pnpm/issues/808
  await execPnpm('install', '-g', '--independent-leaves', 'is-negative')

  const globalPrefix = isWindows()
    ? path.join(global, 'npm', 'pnpm-global', `${LAYOUT_VERSION}_independent_leaves`)
    : path.join(global, 'pnpm-global', `${LAYOUT_VERSION}_independent_leaves`)

  const isPositive = require(path.join(globalPrefix, 'node_modules', 'is-positive'))
  t.ok(typeof isPositive === 'function', 'isPositive() is available')

  const isNegative = require(path.join(globalPrefix, 'node_modules', 'is-negative'))
  t.ok(typeof isNegative === 'function', 'isNegative() is available')
})
