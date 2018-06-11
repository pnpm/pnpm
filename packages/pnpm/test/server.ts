import byline = require('byline')
import { ChildProcess } from 'child_process'
import delay = require('delay')
import { CancelError, PDelayedPassThroughThunk } from 'delay'
import isWindows = require('is-windows')
import fs = require('mz/fs')
import path = require('path')
import pathExists = require('path-exists')
import { Readable } from 'stream';
import tape = require('tape')
import promisifyTape from 'tape-promise'
import killcb = require('tree-kill')
import promisify = require('util.promisify')
import writeJsonFile = require('write-json-file')
import {
  createDeferred,
  Deferred,
  execPnpm,
  execPnpmSync,
  prepare,
  ResolveFunction,
  retryLoadJsonFile,
  spawn,
} from './utils'

// Third element is true if and only if we attempted to kill the process via a signal.
type ServerProcess = [ChildProcess, Deferred<void>, boolean]
const IS_WINDOWS = isWindows()
const test = promisifyTape(tape)
test['only'] = promisifyTape(tape.only)
const kill = promisify(killcb)

test('installation using pnpm server', async (t: tape.Test) => {
  const project = prepare(t)

  const server = spawn(['server', 'start'])

  const serverJsonPath = path.resolve('..', 'store', '2', 'server', 'server.json')
  const serverJson = await retryLoadJsonFile(serverJsonPath)
  t.ok(serverJson)
  t.ok(serverJson.connectionOptions)
  t.equal(typeof serverJson.pnpmVersion, 'string', 'pnpm version added added to server.json')

  await execPnpm('install', 'is-positive@1.0.0')

  t.ok(project.requireModule('is-positive'))

  await execPnpm('uninstall', 'is-positive')

  await execPnpm('server', 'stop')

  t.notOk(await pathExists(serverJsonPath), 'server.json removed')
})

test('store server: headless installation', async (t: tape.Test) => {
  const project = prepare(t)

  const server = spawn(['server', 'start'])

  const serverJsonPath = path.resolve('..', 'store', '2', 'server', 'server.json')
  const serverJson = await retryLoadJsonFile(serverJsonPath)
  t.ok(serverJson)
  t.ok(serverJson.connectionOptions)

  await execPnpm('install', 'is-positive@1.0.0', '--shrinkwrap-only')

  await execPnpm('install', '--frozen-shrinkwrap')

  t.ok(project.requireModule('is-positive'))

  await execPnpm('server', 'stop')

  t.notOk(await pathExists(serverJsonPath), 'server.json removed')
})

test('installation using pnpm server that runs in the background', async (t: tape.Test) => {
  const project = prepare(t)

  await execPnpm('server', 'start', '--background')

  const serverJsonPath = path.resolve('..', 'store', '2', 'server', 'server.json')
  const serverJson = await retryLoadJsonFile(serverJsonPath)
  t.ok(serverJson)
  t.ok(serverJson.connectionOptions)

  await execPnpm('install', 'is-positive@1.0.0')

  t.ok(project.requireModule('is-positive'))

  await execPnpm('uninstall', 'is-positive')

  await execPnpm('server', 'stop')

  t.notOk(await pathExists(serverJsonPath), 'server.json removed')
})

test('installation using pnpm server via TCP', async (t: tape.Test) => {
  const project = prepare(t)

  const server = spawn(['server', 'start', '--protocol', 'tcp'])

  const serverJsonPath = path.resolve('..', 'store', '2', 'server', 'server.json')
  const serverJson = await retryLoadJsonFile(serverJsonPath)
  t.ok(serverJson)
  t.ok(serverJson.connectionOptions.remotePrefix.indexOf('http://localhost:') === 0, 'TCP is used for communication')

  await execPnpm('install', 'is-positive@1.0.0')

  t.ok(project.requireModule('is-positive'))

  await execPnpm('uninstall', 'is-positive')

  await execPnpm('server', 'stop')

  t.notOk(await pathExists(serverJsonPath), 'server.json removed')
})

test('pnpm server uses TCP when port specified', async (t: tape.Test) => {
  const project = prepare(t)

  const server = spawn(['server', 'start', '--port', '7856'])

  const serverJsonPath = path.resolve('..', 'store', '2', 'server', 'server.json')
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

  // TODO: remove the delay and run install by connecting it to the store server
  // Can be done once this gets implemented: https://github.com/pnpm/pnpm/issues/1018
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

  const serverJsonPath = path.resolve('..', 'store', '2', 'server', 'server.json')
  const serverJson = await retryLoadJsonFile(serverJsonPath)
  t.ok(serverJson)
  t.ok(serverJson.connectionOptions)

  t.ok(project.requireModule('is-positive'))

  await execPnpm('uninstall', 'is-positive')

  await execPnpm('server', 'stop')

  t.notOk(await pathExists(serverJsonPath), 'server.json removed')
})

test('store server started in the background should use store location wanted by install', async (t: tape.Test) => {
  const project = prepare(t)

  await execPnpm('install', 'is-positive@1.0.0', '--use-store-server', '--store', '../store2')

  const serverJsonPath = path.resolve('..', 'store2', '2', 'server', 'server.json')
  const serverJson = await retryLoadJsonFile(serverJsonPath)
  t.ok(serverJson)
  t.ok(serverJson.connectionOptions)

  t.ok(project.requireModule('is-positive'))

  await execPnpm('uninstall', 'is-positive', '--store', '../store2')

  await execPnpm('server', 'stop', '--store', '../store2')

  t.notOk(await pathExists(serverJsonPath), 'server.json removed')
})

async function testParallelServerStart (options: {
  test: tape.Test,
  timeoutMillis?: number,
  onProcessClosed?: (serverProcess: ChildProcess, weAttemptedKill: boolean) => void,
  n?: number,
  }) {
  const pnpmBin = 'pnpm'
  const serverProcessList: ServerProcess[] = []
  // Variable that allows us to cancel the timeout.
  let timeoutPromise: PDelayedPassThroughThunk<void>|null = delay(!options.timeoutMillis && options.timeoutMillis !== 0 ? 10000 : options.timeoutMillis)
  // Promise that completes when all server processes have terminated.
  let completedPromise: Promise<void> = Promise.resolve()
  const n = !options.n && options.n !== 0 ? 10 : options.n
  for (let i = 0; i < n; i++) {
    const item: ServerProcess = [
      spawn(['server', 'start']),
      createDeferred<void>(),
      // This is true if and only if we attempted to kill the process via a signal.
      false]
    serverProcessList.push(item)
    byline(item[0].stderr).on('data', (line: Buffer) => options.test.comment(`${item[0].pid}: ${line}`))
    byline(item[0].stdout).on('data', (line: Buffer) => options.test.comment(`${item[0].pid}: ${line}`))
    item[0].on('exit', (code: number|null, signal: string|null) => {
      if (options.onProcessClosed) {
        (options.onProcessClosed)(item[0], item[2])
      }
      for (let j = 0; j < serverProcessList.length; j++) {
        if (serverProcessList[j][0] === item[0]) {
          serverProcessList.splice(j, 1)
          break
        }
      }
      item[1].resolve()
    });
    completedPromise = completedPromise.then(() => item[1].promise)
  }
  await Promise.all([
    (async () => {
      await completedPromise
      // Don't fire timeout if all server processes completed for some reason.
      if (timeoutPromise !== null) {
        timeoutPromise.cancel()
      }
    })(),
    (async () => {
      try {
        await timeoutPromise
      } catch (error) {
        if (error instanceof CancelError) {
          // All child processes completed, don't fire timeout or try to kill processes.
          return
        }
        // This should never happen so it's OK if we don't kill remaining server processes in this path.
        throw error
      }
      timeoutPromise = null
      for (const item of serverProcessList) {
        item[2] = true
      }
      await execPnpm('server', 'stop')
      await Promise.all(serverProcessList.map(async (item) => {
        // Use SIGINT so that the process can delete server.json for the next test.
        // Windows does not support signals: the server process will be killed without
        // it having a chance to perform cleanup, but the 'server stop' will make sure
        // server.json etc. are removed.
        await kill(item[0].pid, 'SIGINT')
        await item[1].promise
      }))
    })(),
  ])
}

test('parallel server starts against the same store should result in only one server process existing after 10 seconds', async (t: tape.Test) => {
  // Number of server processes to start in parallel
  const n = 5
  // Plan that n - 1 of n server processes will close within 10 seconds.
  // +1 for the server.json check.
  // +1 for the assertion in prepare(t).
  // n + 1 total
  t.plan(n + 1)

  const project = prepare(t)
  await testParallelServerStart({
    n,
    onProcessClosed: (serverProcess: ChildProcess, weAttemptedKill: boolean) => {
      if (!weAttemptedKill) {
        t.pass(`the server process ${serverProcess.pid} exited`)
      }
    },
    test: t,
    timeoutMillis: 10000,
  })
  const serverJsonPath = path.resolve('..', 'store', '2', 'server', 'server.json')
  t.notOk(await pathExists(serverJsonPath), 'server.json removed')
})

test('installation without store server running in the background', async (t: tape.Test) => {
  const project = prepare(t)

  await execPnpm('install', 'is-positive@1.0.0', '--no-use-store-server')

  const serverJsonPath = path.resolve('..', 'store', '2', 'server', 'server.json')
  t.notOk(await pathExists(serverJsonPath), 'store server not running')

  t.ok(project.requireModule('is-positive'))
})

test('fail if the store server is run by a different version of pnpm', async (t: tape.Test) => {
  const project = prepare(t)

  const serverJsonPath = path.resolve('..', 'store', '2', 'server', 'server.json')
  await writeJsonFile(serverJsonPath, {pnpmVersion: '2.0.0'})

  const result = execPnpmSync('install', 'is-positive@1.0.0')

  t.equal(result.status, 1)
  t.ok(result.stdout.toString().indexOf('The store server runs on pnpm v2.0.0. The same pnpm version should be used to connect (current is') !== -1)
})
