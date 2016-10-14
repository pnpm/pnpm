import {cleanCache, installPkgs} from '../src'
import globalPath from './support/globalPath'
import tape = require('tape')
import promisifyTape = require('tape-promise')
import exists = require('exists-file')
import path = require('path')

const test = promisifyTape(tape)

test('cache clean removes cache', async function (t) {
  await installPkgs(['is-positive'], {globalPath, global: true})

  const cache = path.join(globalPath, 'cache')

  t.ok(await exists(cache), 'cache is created')

  await cleanCache(globalPath)

  t.ok(!await exists(cache), 'cache is removed')
})
