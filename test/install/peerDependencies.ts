import tape = require('tape')
import promisifyTape from 'tape-promise'
import path = require('path')
import exists = require('path-exists')
import {installPkgs} from '../../src'
import {
  prepare,
  testDefaults,
} from '../utils'

const test = promisifyTape(tape)

test("don't fail when peer dependency is fetched from GitHub", t => {
  const project = prepare(t)
  return installPkgs(['test-pnpm-peer-deps'], testDefaults())
})

test('peer dependency is linked', async t => {
  const project = prepare(t)
  await installPkgs(['ajv@4.10.4', 'ajv-keywords@1.5.0'], testDefaults())

  t.ok(await exists(path.join('node_modules', '.localhost+4873', 'ajv-keywords', '1.5.0', 'node_modules', 'ajv')), 'peer dependency is linked')
})
