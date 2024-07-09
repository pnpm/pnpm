import fs from 'fs'
import { type ChildProcess } from 'child_process'
import { type Readable } from 'stream'
import { promisify } from 'util'
import path from 'path'
import byline from '@pnpm/byline'
import { type Project, prepare } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import delay, { type ClearablePromise } from 'delay'
import pDefer, { type DeferredPromise } from 'p-defer'
import isWindows from 'is-windows'

import killcb from 'tree-kill'
import writeJsonFile from 'write-json-file'
import pAny from 'p-any'
import {
  execPnpm,
  execPnpmSync,
  retryLoadJsonFile,
  spawnPnpm,
} from './utils'

const skipOnWindows = isWindows() ? test.skip : test

// Third element is true if and only if we attempted to kill the process via a signal.
interface ServerProcess {
  childProcess: ChildProcess
  running: DeferredPromise<void>
  attemptedToKill: boolean
}

const kill = promisify(killcb) as (pid: number, signal: string) => Promise<void>

// Polyfilling Symbol.asyncDispose for Jest.
//
// Copied with a few changes from https://devblogs.microsoft.com/typescript/announcing-typescript-5-2/#using-declarations-and-explicit-resource-management
if (Symbol.asyncDispose === undefined) {
  (Symbol as { asyncDispose?: symbol }).asyncDispose = Symbol('Symbol.asyncDispose')
}

interface TestSetup extends AsyncDisposable {
  readonly project: Project
  readonly serverJsonPath: string
}

function prepareServerTest (serverStartArgs?: readonly string[]): TestSetup {
  const project = prepare()

  spawnPnpm(['server', 'start', ...(serverStartArgs ?? [])])
  const serverJsonPath = path.resolve('..', 'store/v3/server/server.json')

  async function onTestEnd () {
    await expect(execPnpm(['server', 'stop'])).resolves.not.toThrow()
    expect(fs.existsSync(serverJsonPath)).toBeFalsy()
  }

  return {
    project,
    serverJsonPath,
    [Symbol.asyncDispose]: onTestEnd,
  }
}

skipOnWindows('installation using pnpm server', async () => {
  await using setup = prepareServerTest()
  const { project, serverJsonPath } = setup

  const serverJson = await retryLoadJsonFile<{ connectionOptions: object, pnpmVersion: string }>(serverJsonPath)
  expect(serverJson).toBeTruthy()
  expect(serverJson.connectionOptions).toBeTruthy()
  expect(typeof serverJson.pnpmVersion).toBe('string')

  await expect(execPnpm(['install', 'is-positive@1.0.0'])).resolves.not.toThrow()

  expect(project.requireModule('is-positive')).toBeTruthy()

  await expect(execPnpm(['uninstall', 'is-positive'])).resolves.not.toThrow()
})

skipOnWindows('store server: headless installation', async () => {
  await using setup = prepareServerTest()
  const { project, serverJsonPath } = setup

  const serverJson = await retryLoadJsonFile<{ connectionOptions: object }>(serverJsonPath)
  expect(serverJson).toBeTruthy()
  expect(serverJson.connectionOptions).toBeTruthy()

  await expect(execPnpm(['install', 'is-positive@1.0.0', '--lockfile-only'])).resolves.not.toThrow()

  await expect(execPnpm(['install', '--frozen-lockfile'])).resolves.not.toThrow()

  expect(project.requireModule('is-positive')).toBeTruthy()
})

skipOnWindows('installation using pnpm server that runs in the background', async () => {
  await using setup = prepareServerTest(['--background'])
  const { project, serverJsonPath } = setup

  const serverJson = await retryLoadJsonFile<{ connectionOptions: object }>(serverJsonPath)
  expect(serverJson).toBeTruthy()
  expect(serverJson.connectionOptions).toBeTruthy()

  await expect(execPnpm(['install', 'is-positive@1.0.0'])).resolves.not.toThrow()

  expect(project.requireModule('is-positive')).toBeTruthy()

  await expect(execPnpm(['uninstall', 'is-positive'])).resolves.not.toThrow()
})

skipOnWindows('installation using pnpm server via TCP', async () => {
  await using setup = prepareServerTest(['--protocol', 'tcp'])
  const { project, serverJsonPath } = setup

  const serverJson = await retryLoadJsonFile<{ connectionOptions: { remotePrefix: string } }>(serverJsonPath)
  expect(serverJson).toBeTruthy()
  expect(serverJson.connectionOptions.remotePrefix.indexOf('http://localhost:')).toBe(0) // TCP is used for communication'

  await expect(execPnpm(['install', 'is-positive@1.0.0'])).resolves.not.toThrow()

  expect(project.requireModule('is-positive')).toBeTruthy()

  await expect(execPnpm(['uninstall', 'is-positive'])).resolves.not.toThrow()
})

skipOnWindows('pnpm server uses TCP when port specified', async () => {
  await using setup = prepareServerTest(['--port', '7856'])
  const { serverJsonPath } = setup

  const serverJson = await retryLoadJsonFile<{ connectionOptions: { remotePrefix: string } }>(serverJsonPath)
  expect(serverJson).toBeTruthy()
  expect(serverJson.connectionOptions.remotePrefix).toBe('http://localhost:7856') // TCP with specified port is used for communication
})

test.skip('pnpm server fails when trying to set --port for IPC protocol', async () => {
  prepare()

  expect(execPnpmSync(['server', 'start', '--protocol', 'ipc', '--port', '7856']).status).toBe(1)
})

test('stopping server fails when the server disallows stopping via remote call', async () => {
  prepare()

  const server = spawnPnpm(['server', 'start', '--ignore-stop-requests'])

  await delay(2000) // lets' wait till the server starts

  expect(execPnpmSync(['server', 'stop']).status).toBe(1)

  await kill(server.pid!, 'SIGINT')
})

skipOnWindows('uploading cache can be disabled without breaking install', async () => {
  await using setup = prepareServerTest(['--ignore-upload-requests'])
  const { project } = setup

  // TODO: remove the delay and run install by connecting it to the store server
  // Can be done once this gets implemented: https://github.com/pnpm/pnpm/issues/1018
  await delay(2000)

  // install a package that has side effects
  await expect(execPnpm(['add', '--side-effects-cache', 'diskusage@1.1.3'])).resolves.not.toThrow()

  // make sure the installation is successful, but the cache has not been written
  project.has('diskusage')
  const storePath = project.getStorePath()
  const engine = `${process.platform}-${process.arch}-node-${process.version.split('.')[0]}`
  const cacheDir = path.join(storePath, `localhost+${REGISTRY_MOCK_PORT}/diskusage/1.1.3/side_effects/${engine}/package`)
  expect(fs.existsSync(cacheDir)).toBeFalsy()
})

skipOnWindows('installation using store server started in the background', async () => {
  const project = prepare()

  await expect(execPnpm(['install', 'is-positive@1.0.0', '--use-store-server'])).resolves.not.toThrow()

  const serverJsonPath = path.resolve('..', 'store/v3/server/server.json')

  try {
    const serverJson = await retryLoadJsonFile<{ connectionOptions: object }>(serverJsonPath)
    expect(serverJson).toBeTruthy()
    expect(serverJson.connectionOptions).toBeTruthy()

    expect(project.requireModule('is-positive')).toBeTruthy()

    await expect(execPnpm(['uninstall', 'is-positive'])).resolves.not.toThrow()
  } finally {
    await expect(execPnpm(['server', 'stop'])).resolves.not.toThrow()
    expect(fs.existsSync(serverJsonPath)).toBeFalsy()
  }
})

skipOnWindows('store server started in the background should use store location wanted by install', async () => {
  const project = prepare()

  await expect(execPnpm(['add', 'is-positive@1.0.0', '--use-store-server', '--store-dir', '../store2'])).resolves.not.toThrow()

  const serverJsonPath = path.resolve('..', 'store2/v3/server/server.json')

  try {
    const serverJson = await retryLoadJsonFile<{ connectionOptions: object }>(serverJsonPath)
    expect(serverJson).toBeTruthy()
    expect(serverJson.connectionOptions).toBeTruthy()

    expect(project.requireModule('is-positive')).toBeTruthy()

    await expect(execPnpm(['remove', 'is-positive', '--store-dir', '../store2'])).resolves.not.toThrow()
  } finally {
    await expect(execPnpm(['server', 'stop', '--store-dir', '../store2'])).resolves.not.toThrow()
    expect(fs.existsSync(serverJsonPath)).toBeFalsy()
  }
})

async function testParallelServerStart (
  options: {
    timeoutMillis?: number
    onProcessClosed: (serverProcess: ChildProcess, weAttemptedKill: boolean) => void
    n: number
  }
) {
  let stopped = false
  const serverProcessList: ServerProcess[] = []

  // Promise that completes when all server processes have terminated.
  let completedPromise: Promise<void> = Promise.resolve()
  for (let i = 0; i < options.n; i++) {
    const item: ServerProcess = {
      attemptedToKill: false,
      childProcess: spawnPnpm(['server', 'start']),
      running: pDefer<undefined>(),
      // This is true if and only if we attempted to kill the process via a signal.
    }
    serverProcessList.push(item)

    byline(item.childProcess.stderr as Readable).on('data', (line: Buffer) => {
      console.log(`${item.childProcess.pid?.toString() ?? ''}: ${line.toString()}`)
    })
    byline(item.childProcess.stdout as Readable).on('data', (line: Buffer) => {
      console.log(`${item.childProcess.pid?.toString() ?? ''}: ${line.toString()}`)
    })

    item.childProcess.on('exit', async (code: number | null, signal: string | null) => {
      options.onProcessClosed(item.childProcess, item.attemptedToKill)
      for (let j = 0; j < serverProcessList.length; j++) {
        if (serverProcessList[j].childProcess === item.childProcess) {
          serverProcessList.splice(j, 1)
          break
        }
      }
      item.running.resolve()
      if (serverProcessList.length === 1 && timeoutPromise !== null) {
        serverProcessList[0].attemptedToKill = true
        await stopRemainingServers()
      }
    })
    completedPromise = completedPromise.then(async () => item.running.promise)
  }

  const timeoutMillis = options.timeoutMillis ?? 10000
  let timeoutPromise: ClearablePromise<void> | null = delay(timeoutMillis)
  await pAny([
    (async () => {
      await completedPromise
      // Don't fire timeout if all server processes completed for some reason.
      if (timeoutPromise !== null) {
        timeoutPromise.clear()
      }
    })(),
    (async () => {
      await timeoutPromise
      timeoutPromise = null
      for (const item of serverProcessList) {
        item.attemptedToKill = true
      }
      await stopRemainingServers()
    })(),
  ])

  async function stopRemainingServers () {
    if (stopped) return
    stopped = true
    await execPnpm(['server', 'stop'])
    await Promise.all(serverProcessList.map(async (item) => {
      // Use SIGINT so that the process can delete server.json for the next test.
      // Windows does not support signals: the server process will be killed without
      // it having a chance to perform cleanup, but the 'server stop' will make sure
      // server.json etc. are removed.
      await kill(item.childProcess.pid!, 'SIGINT')
      await item.running.promise
    }))
  }
}

skipOnWindows('parallel server starts against the same store should result in only one server process existing after 10 seconds', async () => {
  // Number of server processes to start in parallel
  const n = 5
  // Plan that n - 1 of n server processes will close within 10 seconds.
  // +1 for the server.json check.
  // +1 for the testParallelServerStart promise resolve
  // n + 1 total
  expect.assertions(n + 1)

  prepare()
  await expect(testParallelServerStart({
    n,
    onProcessClosed: (serverProcess: ChildProcess, weAttemptedKill: boolean) => {
      if (!weAttemptedKill) {
        console.log(`the server process ${serverProcess.pid?.toString() ?? ''} exited`)
        expect(1).toBe(1)
      }
    },
    timeoutMillis: 60000,
  })).resolves.not.toThrow()
  const serverJsonPath = path.resolve('..', 'store/v3/server/server.json')
  expect(fs.existsSync(serverJsonPath)).toBeFalsy()
})

skipOnWindows('installation without store server running in the background', async () => {
  const project = prepare()

  await expect(execPnpm(['install', 'is-positive@1.0.0', '--no-use-store-server'])).resolves.not.toThrow()

  const serverJsonPath = path.resolve('..', 'store/v3/server/server.json')
  expect(fs.existsSync(serverJsonPath)).toBeFalsy()

  expect(project.requireModule('is-positive')).toBeTruthy()
})

// Failing would create issues for glitch.com
// per @etamponi:
// > I update it on the host, which triggers a restart of the pnpm server,
//   and then I update it on the container images, but that doesn't restart the running containers
test.skip('fail if the store server is run by a different version of pnpm', async () => {
  prepare()

  const serverJsonPath = path.resolve('..', 'store/v3/server/server.json')
  writeJsonFile.sync(serverJsonPath, { pnpmVersion: '2.0.0' })

  const result = execPnpmSync(['install', 'is-positive@1.0.0'])

  expect(result.status).toBe(1)
  expect(result.stdout.toString()).toMatch(/The store server runs on pnpm v2.0.0. The same pnpm version should be used to connect \(current is/)
})

skipOnWindows('print server status', async () => {
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  await using _setup = prepareServerTest()

  await delay(2000)

  const result = execPnpmSync(['server', 'status', '--store-dir', path.resolve('..', 'store')])

  expect(result.status).toBe(0)
  const output = result.stdout.toString()
  expect(output).toContain('process id: ')
})

test('fail if no store server is running and --use-running-store-server flag is used', async () => {
  prepare()

  const result = execPnpmSync(['install', 'is-positive', '--use-running-store-server', '--store-dir', path.resolve('..', 'store')])

  expect(result.status).toBe(1)
  const output = result.stdout.toString()
  expect(output).toContain('No store server is running.')
})
