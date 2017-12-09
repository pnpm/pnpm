import delay = require('delay')
import isWindows = require('is-windows')
import path = require('path')
import fs = require('mz/fs')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import killcb = require('tree-kill')
import loadJsonFile = require('load-json-file')
import pathExists = require('path-exists')
import thenify = require('thenify')
import {
  prepare,
  execPnpm,
  spawn,
} from './utils'

const IS_WINDOWS = isWindows()
const test = promisifyTape(tape)
const kill = thenify(killcb)

test('installation using pnpm server', async (t: tape.Test) => {
  const project = prepare(t)

  const server = spawn(['server'])

  await delay(2000) // lets' wait till the server starts

  const serverJsonPath = path.resolve('..', 'store', '2', 'server.json')
  const serverJson = await loadJsonFile(serverJsonPath)
  t.ok(serverJson)
  t.ok(serverJson.connectionOptions)

  await execPnpm('install', 'is-positive@1.0.0')

  t.ok(project.requireModule('is-positive'))

  await execPnpm('uninstall', 'is-positive')

  await execPnpm('store', 'prune')

  // we don't actually know when the server will prune the store
  // lets' just wait a bit before checking
  await delay(1000)

  await project.storeHasNot('is-positive', '1.0.0')

  await kill(server.pid, 'SIGINT')

  // TODO: fix this test for Windows
  if (!IS_WINDOWS) {
    await delay(2000) // lets' wait till the server starts

    t.notOk(await pathExists(serverJsonPath), 'server.json removed')
  }
})
