import prepare from '@pnpm/prepare'
import promisifyTape from 'tape-promise'
import {
  execPnpm,
  retryLoadJsonFile,
  spawnPnpm,
} from '../utils'
import path = require('path')
import pathExists = require('path-exists')
import tape = require('tape')

const test = promisifyTape(tape)

test('self-update stops the store server', async (t: tape.Test) => {
  prepare(t)

  spawnPnpm(['server', 'start'])

  const serverJsonPath = path.resolve('../store/v3/server/server.json')
  const serverJson = await retryLoadJsonFile<{ connectionOptions: object }>(serverJsonPath)
  t.ok(serverJson)
  t.ok(serverJson.connectionOptions)

  const global = path.resolve('global')

  const env = { NPM_CONFIG_PREFIX: global }
  if (process.env.APPDATA) env['APPDATA'] = global

  await execPnpm(['install', '-g', 'pnpm', '--store-dir', path.resolve('..', 'store')], { env })

  t.notOk(await pathExists(serverJsonPath), 'server.json removed')
})
