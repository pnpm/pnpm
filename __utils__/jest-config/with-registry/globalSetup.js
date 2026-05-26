import { spawn } from 'node:child_process'
import { createRequire } from 'node:module'
import { scheduler } from 'node:timers/promises'
import { promisify } from 'node:util'

import getPort from 'get-port'
import { readYamlFileSync } from 'read-yaml-file'
import treeKill from 'tree-kill'

const kill = promisify(treeKill)
const require = createRequire(import.meta.url)

export default async () => {
  if (!process.env.PNPM_REGISTRY_MOCK_PORT) {
    process.env.PNPM_REGISTRY_MOCK_PORT = (await getPort({ from: 7700, to: 7800 })).toString()
  }

  // We still call `prepare()` from `@pnpm/registry-mock`: it copies
  // the read-only fixture `storage-cache` into a tempy directory
  // and writes `registry/runtime-config-${port}.yaml` with the
  // tempy path under `storage:`. That yaml is what
  // `locations.storage()` reads when `getIntegrity` (also from
  // registry-mock) is called from tests. We just don't launch
  // verdaccio against it — we launch pnpm-registry instead.
  const { prepare, REGISTRY_MOCK_CREDENTIALS } = await import('@pnpm/registry-mock')
  const { addUser } = await import('@pnpm/testing.registry-mock')
  prepare()

  const storage = readStoragePath(process.env.PNPM_REGISTRY_MOCK_PORT)
  const bin = resolvePnpmRegistryBin()

  const server = spawn(
    bin,
    [
      '--listen', `127.0.0.1:${process.env.PNPM_REGISTRY_MOCK_PORT}`,
      '--storage', storage,
      '--public-url', `http://localhost:${process.env.PNPM_REGISTRY_MOCK_PORT}`,
      // Match registry-mock's verdaccio config: a one-year TTL so
      // the fixture packuments (mtime: whenever the npm tarball was
      // built) never look stale and never trigger a re-fetch to
      // npmjs.org that would 404.
      '--packument-ttl-secs', '31536000',
    ],
    { stdio: 'inherit' }
  )

  let killed = false
  server.on('error', (err) => {
    console.log(err)
  })
  server.on('close', () => {
    if (!killed) {
      console.log('Error: The registry server was killed!')
      process.exit(1)
    }
  })
  global.killServer = () => {
    killed = true
    return kill(server.pid)
  }

  await waitForServerOnline()

  // Register the test user and store the auth token for bearer-based tests
  const { token } = await addUser({
    username: REGISTRY_MOCK_CREDENTIALS.username,
    password: REGISTRY_MOCK_CREDENTIALS.password,
    email: 'foo@bar.net',
  })
  process.env.REGISTRY_MOCK_TOKEN = token
}

/**
 * Read the `storage:` path that `@pnpm/registry-mock`'s `prepare()`
 * just wrote into the runtime-config yaml. We can't import
 * `locations.storage` from the registry-mock package — it isn't
 * re-exported from its `index.ts` — but the file path is stable.
 */
function readStoragePath (port) {
  const configPath = require.resolve(
    `@pnpm/registry-mock/registry/runtime-config-${port}.yaml`
  )
  const { storage } = readYamlFileSync(configPath)
  return storage
}

/**
 * Locate the `pnpm-registry` binary. Lookup order:
 *
 * 1. `PNPM_REGISTRY_BIN` env var override (escape hatch for local
 *    Rust work — point it at `target/release/pnpm-registry` to test
 *    in-progress changes to the registry crate).
 * 2. The platform binary that `pnpm install` pulled in as an
 *    optionalDependency of `@pnpm/pnpr` — i.e.
 *    `@pnpm/pnpr.<platform>-<arch>/pnpr[.exe]`. The resolution goes
 *    through the `@pnpm/pnpr` wrapper's path because the platform
 *    sub-package lives in the wrapper's own `node_modules`, not
 *    anywhere on the parent chain of this file.
 */
function resolvePnpmRegistryBin () {
  if (process.env.PNPM_REGISTRY_BIN) {
    return process.env.PNPM_REGISTRY_BIN
  }
  const ext = process.platform === 'win32' ? '.exe' : ''
  const platformPkg = `@pnpm/pnpr.${process.platform}-${process.arch}`
  try {
    const wrapperRequire = createRequire(require.resolve('@pnpm/pnpr/bin/pnpr'))
    return wrapperRequire.resolve(`${platformPkg}/pnpr${ext}`)
  } catch {
    throw new Error(
      `pnpm-registry binary not found. The test suite expects ${platformPkg} ` +
      'to be installed (it ships as an optionalDependency of @pnpm/pnpr — ' +
      'run `pnpm install` at the repo root to pull it in), or set ' +
      'PNPM_REGISTRY_BIN to an absolute path to a locally-built binary.'
    )
  }
}

const UNUSUAL_REGISTRY_STARTUP_THRESHOLD = 15 // seconds

async function waitForServerOnline () {
  const start = performance.now()

  for (const delay of exponentialBackoff()) {
    try {
      await fetch(`http://localhost:${process.env.PNPM_REGISTRY_MOCK_PORT}`, { method: 'HEAD' })

      const totalWait = (performance.now() - start) / 1000
      if (totalWait > UNUSUAL_REGISTRY_STARTUP_THRESHOLD) {
        console.warn(`pnpm-registry required an unusually long amount of time to start: ${totalWait} seconds`)
      }

      return
    } catch (err) {
      // If pnpm-registry hasn't begun listening yet, attempts to
      // connect to the unbound port should throw ECONNREFUSED. If a different
      // error is observed, throw an error.
      if (err?.cause?.code !== 'ECONNREFUSED') {
        throw new Error('Failed to bring pnpm-registry online:', { cause: err })
      }

      await scheduler.wait(delay)
    }
  }

  const totalWait = (performance.now() - start) / 1000
  throw new Error(`pnpm-registry did not come online after waiting ${totalWait} seconds`)
}

function *exponentialBackoff (attempts = 15, base = 1.5, initialWait = 100) {
  for (let i = 0; i < attempts; i++) {
    yield initialWait * Math.pow(base, i)
  }
}

