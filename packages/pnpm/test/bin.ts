import { tempDir } from '@pnpm/prepare'
import promisifyTape from 'tape-promise'
import { execPnpmSync } from './utils'
import path = require('path')
import fs = require('mz/fs')
import PATH = require('path-name')
import tape = require('tape')

const test = promisifyTape(tape)

test('pnpm bin', async (t: tape.Test) => {
  tempDir(t)
  await fs.mkdir('node_modules')

  const result = execPnpmSync(['bin'])

  t.equal(result.status, 0)
  t.equal(result.stdout.toString(), path.resolve('node_modules/.bin'))
})

test('pnpm bin -g', async (t: tape.Test) => {
  tempDir(t)

  const result = execPnpmSync(['bin', '-g'])

  t.equal(result.status, 0)
  t.ok(process.env[PATH].includes(result.stdout.toString()))
})
