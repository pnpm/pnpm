import tape = require('tape')
import promisifyTape from 'tape-promise'
import path = require('path')
import {prepare, testDefaults} from '../utils'
import {installPkgs} from '../../src'

const test = promisifyTape(tape)

test('global installation', async function (t) {
  prepare(t)
  const globalPrefix = path.resolve('..', 'global')
  const opts = testDefaults({global: true, prefix: globalPrefix})
  await installPkgs(['is-positive'], opts)

  // there was an issue when subsequent installations were removing everything installed prior
  // https://github.com/pnpm/pnpm/issues/808
  await installPkgs(['is-negative'], opts)

  const isPositive = require(path.join(globalPrefix, 'node_modules', 'is-positive'))
  t.ok(typeof isPositive === 'function', 'isPositive() is available')

  const isNegative = require(path.join(globalPrefix, 'node_modules', 'is-negative'))
  t.ok(typeof isNegative === 'function', 'isNegative() is available')
})
