import { scheduler } from 'node:timers/promises'
import getPort from 'get-port'
import { promisify } from 'util'
import treeKill from 'tree-kill'
const kill = promisify(treeKill)

export default async () => {
  if (!process.env.PNPM_REGISTRY_MOCK_PORT) {
    process.env.PNPM_REGISTRY_MOCK_PORT = (await getPort({ from: 7700, to: 7800 })).toString()
  }
  const { start, prepare } = await import('@pnpm/registry-mock')
  prepare()
  const server = start({
    // Verdaccio stopped working properly on Node.js 22.
    // You can test the issue by running:
    //   pnpm --filter=core run test test/install/auth.ts
    useNodeVersion: '20.16.0',
    stdio: 'inherit',
    listen: process.env.PNPM_REGISTRY_MOCK_PORT,
  })
  let killed = false
  server.on('error', (err) => {
    console.log(err)
  })
  let forceExit = false
  process.on('SIGTERM', () => {
    forceExit = true
  })

  server.on('close', () => {
    if (!killed && !forceExit) {
      console.log('Warning: The registry server was killed unexpectedly')
    }
  })
  global.killServer = () => {
    killed = true
    return kill(server.pid)
  }

  // Verdaccio can take a bit of time to come online on Windows and during its
  // first startup. Some tests will fail immediately if they begin running
  // before Verdaccio starts. Wait for Verdaccio to become online before running
  // any tests.
  await waitForServerOnline()

  // Register the test user and store the auth token for bearer-based tests
  const { addUser, REGISTRY_MOCK_CREDENTIALS } = await import('@pnpm/registry-mock')
  const { token } = await addUser({
    username: REGISTRY_MOCK_CREDENTIALS.username,
    password: REGISTRY_MOCK_CREDENTIALS.password,
    email: 'foo@bar.net',
  })
  process.env.REGISTRY_MOCK_TOKEN = token
}

const UNUSUAL_VERDACCIO_STARTUP_THRESHOLD = 15 // seconds

async function waitForServerOnline () {
  const start = performance.now()

  for (const delay of exponentialBackoff()) {
    try {
      await fetch(`http://localhost:${process.env.PNPM_REGISTRY_MOCK_PORT}`, { method: 'HEAD' })

      const totalWait = (performance.now() - start) / 1000
      if (totalWait > UNUSUAL_VERDACCIO_STARTUP_THRESHOLD) {
        console.warn(`Verdaccio required an unusually long amount of time to start: ${totalWait} seconds`)
      }

      return
    } catch (err) {
      // If the Verdaccio process hasn't begun listening yet, attempts to
      // connect to the unbound port should throw ECONNREFUSED. If a different
      // error is observed, throw an error.
      if (err?.cause?.code !== 'ECONNREFUSED') {
        throw new Error('Failed to bring Verdaccio online:', { cause: err })
      }

      await scheduler.wait(delay)
    }
  }

  const totalWait = (performance.now() - start) / 1000
  throw new Error(`Verdaccio did not come online after waiting ${totalWait} seconds`)
}

function *exponentialBackoff (attempts = 15, base = 1.5, initialWait = 100) {
  for (let i = 0; i < attempts; i++) {
    yield initialWait * Math.pow(base, i)
  }
}
