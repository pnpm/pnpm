import { LAYOUT_VERSION } from '@pnpm/constants'
import { tempDir } from '@pnpm/prepare'
import promisifyTape from 'tape-promise'
import { execPnpmSync } from './utils'
import path = require('path')
import isWindows = require('is-windows')
import tape = require('tape')

const test = promisifyTape(tape)

test('pnpm root', async (t: tape.Test) => {
  tempDir(t)

  const result = execPnpmSync(['root'])

  t.equal(result.status, 0)

  t.equal(result.stdout.toString(), path.resolve('node_modules') + '\n')
})

test('pnpm root -g', async (t: tape.Test) => {
  tempDir(t)

  const global = path.resolve('global')

  const env = { NPM_CONFIG_PREFIX: global }
  if (process.env.APPDATA) env['APPDATA'] = global

  const result = execPnpmSync(['root', '-g'], { env })

  t.equal(result.status, 0)

  if (isWindows()) {
    t.equal(result.stdout.toString(), path.join(global, `npm/pnpm-global/${LAYOUT_VERSION}/node_modules`) + '\n')
  } else {
    t.equal(result.stdout.toString(), path.join(global, `pnpm-global/${LAYOUT_VERSION}/node_modules`) + '\n')
  }
})
