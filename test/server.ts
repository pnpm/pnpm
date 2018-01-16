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
  execPnpmSync,
  spawn,
} from './utils'

const IS_WINDOWS = isWindows()
const test = promisifyTape(tape)
const kill = thenify(killcb)

test('installation using pnpm server', async (t: tape.Test) => {
  const project = prepare(t)

  const server = spawn(['server', 'start'])

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

  await execPnpm('server', 'stop')

  t.notOk(await pathExists(serverJsonPath), 'server.json removed')
})

test('installation using pnpm server via TCP', async (t: tape.Test) => {
  const project = prepare(t)

  const server = spawn(['server', 'start', '--protocol', 'tcp'])

  await delay(2000) // lets' wait till the server starts

  const serverJsonPath = path.resolve('..', 'store', '2', 'server.json')
  const serverJson = await loadJsonFile(serverJsonPath)
  t.ok(serverJson)
  t.ok(serverJson.connectionOptions.remotePrefix.indexOf('http://localhost:') === 0, 'TCP is used for communication')

  await execPnpm('install', 'is-positive@1.0.0')

  t.ok(project.requireModule('is-positive'))

  await execPnpm('uninstall', 'is-positive')

  await execPnpm('store', 'prune')

  // we don't actually know when the server will prune the store
  // lets' just wait a bit before checking
  await delay(1000)

  await project.storeHasNot('is-positive', '1.0.0')

  await execPnpm('server', 'stop')

  t.notOk(await pathExists(serverJsonPath), 'server.json removed')
})

test('pnpm server uses TCP when port specified', async (t: tape.Test) => {
  const project = prepare(t)

  const server = spawn(['server', 'start', '--port', '7856'])

  await delay(2000) // lets' wait till the server starts

  const serverJsonPath = path.resolve('..', 'store', '2', 'server.json')
  const serverJson = await loadJsonFile(serverJsonPath)
  t.ok(serverJson)
  t.equal(serverJson.connectionOptions.remotePrefix, 'http://localhost:7856', 'TCP with specified port is used for communication')

  await execPnpm('server', 'stop')

  t.notOk(await pathExists(serverJsonPath), 'server.json removed')
})

test('pnpm server fails when trying to set --port for IPC protocol', async (t: tape.Test) => {
  const project = prepare(t)

  t.equal(execPnpmSync('server', 'start', '--protocol', 'ipc', '--port', '7856').status, 1, 'process failed')
})

test('stopping server fails when the server disallows stopping via remote call', async (t: tape.Test) => {
  const project = prepare(t)

  const server = spawn(['server', 'start', '--ignore-stop-requests'])

  await delay(2000) // lets' wait till the server starts

  t.equal(execPnpmSync('server', 'stop').status, 1, 'process failed')

  await kill(server.pid, 'SIGINT')
})
