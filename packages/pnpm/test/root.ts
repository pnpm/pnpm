import tape = require('tape')
import promisifyTape from 'tape-promise'
import path = require('path')
import isWindows = require('is-windows')
import {
  execPnpm,
  execPnpmSync,
  tempDir,
} from './utils'

const test = promisifyTape(tape)

test('pnpm root', async (t: tape.Test) => {
  tempDir(t)

  const result = execPnpmSync('root')

  t.equal(result.status, 0)

  t.equal(result.stdout.toString(), path.resolve('node_modules') + '\n')
})

test('pnpm root -g', async (t: tape.Test) => {
  tempDir(t)

  const global = path.resolve('global')

  if (process.env.APPDATA) process.env.APPDATA = global
  process.env.NPM_CONFIG_PREFIX = global

  const result = execPnpmSync('root', '-g')

  t.equal(result.status, 0)

  if (isWindows()) {
    t.equal(result.stdout.toString(), path.join(global, 'npm', 'pnpm-global', '1', 'node_modules') + '\n')
  } else {
    t.equal(result.stdout.toString(), path.join(global, 'pnpm-global', '1', 'node_modules') + '\n')
  }
})

test('pnpm root -g --independent-leaves', async (t: tape.Test) => {
  tempDir(t)

  const global = path.resolve('global')

  if (process.env.APPDATA) process.env.APPDATA = global
  process.env.NPM_CONFIG_PREFIX = global

  const result = execPnpmSync('root', '-g', '--independent-leaves')

  t.equal(result.status, 0)

  if (isWindows()) {
    t.equal(result.stdout.toString(), path.join(global, 'npm', 'pnpm-global', '1_independent_leaves', 'node_modules') + '\n')
  } else {
    t.equal(result.stdout.toString(), path.join(global, 'pnpm-global', '1_independent_leaves', 'node_modules') + '\n')
  }
})
