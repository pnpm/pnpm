import delay = require('delay')
import isWindows = require('is-windows')
import path = require('path')
import fs = require('mz/fs')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import killcb = require('tree-kill')
import pathExists = require('path-exists')
import promisify = require('util.promisify')
import {
  prepare,
  execPnpm,
  execPnpmSync,
  spawn,
  retryLoadJsonFile,
} from './utils'

const IS_WINDOWS = isWindows()
const test = promisifyTape(tape)
test.only = promisifyTape(tape.only)
const kill = promisify(killcb)

test('installation using pnpm server', async (t: tape.Test) => {
  const project = prepare(t)

  const server = spawn(['server', 'start'])

  const serverJsonPath = path.resolve('..', 'store', '2', 'server.json')
  const serverJson = await retryLoadJsonFile(serverJsonPath)
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

test('installation using pnpm server that runs in the background', async (t: tape.Test) => {
  const project = prepare(t)

  await execPnpm('server', 'start', '--background')

  const serverJsonPath = path.resolve('..', 'store', '2', 'server.json')
  const serverJson = await retryLoadJsonFile(serverJsonPath)
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

  const serverJsonPath = path.resolve('..', 'store', '2', 'server.json')
  const serverJson = await retryLoadJsonFile(serverJsonPath)
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

  const serverJsonPath = path.resolve('..', 'store', '2', 'server.json')
  const serverJson = await retryLoadJsonFile(serverJsonPath)
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

test('uploading cache can be disabled without breaking install', async (t: tape.Test) => {
  const project = prepare(t)

  const server = spawn(['server', 'start', '--ignore-upload-requests'])

  await delay(2000)

  // install a package that has side effects
  await execPnpm('install', '--side-effects-cache', 'runas@3.1.1')

  // make sure the installation is successful, but the cache has not been written
  await project.has('runas')
  const storePath = await project.getStorePath()
  const engine = `${process.platform}-${process.arch}-node-${process.version.split('.')[0]}`
  const cacheDir = path.join(storePath, 'localhost+4873', 'runas', '3.1.1', 'side_effects', engine, 'package')
  t.notOk(await pathExists(cacheDir), 'side effects cache not uploaded')

  await execPnpm('server', 'stop')
})

test('installation using store server started in the background', async (t: tape.Test) => {
  const project = prepare(t)

  await execPnpm('install', 'is-positive@1.0.0', '--use-store-server')

  const serverJsonPath = path.resolve('..', 'store', '2', 'server.json')
  const serverJson = await retryLoadJsonFile(serverJsonPath)
  t.ok(serverJson)
  t.ok(serverJson.connectionOptions)

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

test('installation without store server running in the background', async (t: tape.Test) => {
  const project = prepare(t)

  await execPnpm('install', 'is-positive@1.0.0', '--no-use-store-server')

  const serverJsonPath = path.resolve('..', 'store', '2', 'server.json')
  t.notOk(await pathExists(serverJsonPath), 'store server not running')

  t.ok(project.requireModule('is-positive'))
})
