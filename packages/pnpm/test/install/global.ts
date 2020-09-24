import { LAYOUT_VERSION } from '@pnpm/constants'
import prepare from '@pnpm/prepare'
import promisifyTape from 'tape-promise'
import {
  addDistTag,
  execPnpm,
} from '../utils'
import path = require('path')
import isWindows = require('is-windows')
import exists = require('path-exists')
import tape = require('tape')

const test = promisifyTape(tape)

test('global installation', async (t: tape.Test) => {
  prepare(t)
  const global = path.resolve('..', 'global')

  const env = { NPM_CONFIG_PREFIX: global }
  if (process.env.APPDATA) env['APPDATA'] = global

  await execPnpm(['install', '--global', 'is-positive'], { env })

  // there was an issue when subsequent installations were removing everything installed prior
  // https://github.com/pnpm/pnpm/issues/808
  await execPnpm(['install', '--global', 'is-negative'], { env })

  const globalPrefix = path.join(global, `pnpm-global/${LAYOUT_VERSION}`)

  const isPositive = await import(path.join(globalPrefix, 'node_modules', 'is-positive'))
  t.ok(typeof isPositive === 'function', 'isPositive() is available')

  const isNegative = await import(path.join(globalPrefix, 'node_modules', 'is-negative'))
  t.ok(typeof isNegative === 'function', 'isNegative() is available')
})

test('global installation to custom directory with --global-dir', async (t: tape.Test) => {
  prepare(t)

  await execPnpm(['add', '--global', '--global-dir=../global', 'is-positive'])

  const isPositive = await import(path.resolve(`../global/${LAYOUT_VERSION}/node_modules/is-positive`))
  t.ok(typeof isPositive === 'function', 'isPositive() is available')
})

test('always install latest when doing global installation without spec', async (t: tape.Test) => {
  prepare(t)
  await addDistTag('peer-c', '2.0.0', 'latest')

  const global = path.resolve('..', 'global')

  const env = { NPM_CONFIG_PREFIX: global }

  if (process.env.APPDATA) env['APPDATA'] = global

  await execPnpm(['install', '-g', 'peer-c@1'], { env })
  await execPnpm(['install', '-g', 'peer-c'], { env })

  const globalPrefix = path.join(global, `pnpm-global/${LAYOUT_VERSION}`)

  process.chdir(globalPrefix)

  // eslint-disable-next-line @typescript-eslint/no-var-requires
  t.equal(require(path.resolve('node_modules', 'peer-c', 'package.json')).version, '2.0.0')
})

test('run lifecycle events of global packages in correct working directory', async (t: tape.Test) => {
  if (isWindows()) {
    // Skipping this test on Windows because "$npm_execpath run create-file" will fail on Windows
    return
  }

  prepare(t)
  const global = path.resolve('..', 'global')

  const env = { NPM_CONFIG_PREFIX: global }
  if (process.env.APPDATA) env['APPDATA'] = global

  await execPnpm(['install', '-g', 'postinstall-calls-pnpm@1.0.0'], { env })

  t.ok(await exists(path.join(global, `pnpm-global/${LAYOUT_VERSION}/node_modules/postinstall-calls-pnpm/created-by-postinstall`)))
})
