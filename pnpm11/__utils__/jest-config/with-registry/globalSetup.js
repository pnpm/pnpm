import { spawn, spawnSync } from 'node:child_process'
import { existsSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { scheduler } from 'node:timers/promises'
import { promisify } from 'node:util'

import getPort from 'get-port'
import treeKill from 'tree-kill'

const kill = promisify(treeKill)

const REPO_ROOT = path.join(import.meta.dirname, '..', '..', '..', '..')
const FIXTURE_PACKAGES = path.join(REPO_ROOT, 'pnpr', '.fixtures', 'packages')

export default async () => {
  if (!process.env.PNPM_REGISTRY_MOCK_PORT) {
    process.env.PNPM_REGISTRY_MOCK_PORT = (await getPort({ from: 7700, to: 7800 })).toString()
  }

  const { addUser, REGISTRY_MOCK_CREDENTIALS } = await import('@pnpm/testing.registry-mock')

  // Build verdaccio-shaped storage from the in-repo package fixtures. The
  // registry mutates this storage during tests (publishes, dist-tags), so it
  // gets its own writable copy in a temp dir, never the read-only fixtures.
  const storage = mkdtempSync(path.join(tmpdir(), 'pnpm-registry-mock-storage-'))
  buildStorage(storage)
  process.env.PNPM_REGISTRY_MOCK_STORAGE = storage
  const config = writeTestConfig(storage)

  const bin = resolvePnprBin()

  const server = spawn(
    bin,
    [
      '--config', config,
      '--listen', `127.0.0.1:${process.env.PNPM_REGISTRY_MOCK_PORT}`,
      '--storage', storage,
      '--public-url', `http://localhost:${process.env.PNPM_REGISTRY_MOCK_PORT}`,
      // A one-year TTL so the fixture packuments (whose `time` values are
      // static) never look stale and never trigger a re-fetch to
      // npmjs.org that would 404.
      '--packument-ttl-secs', '31536000',
    ],
    { stdio: 'inherit' }
  )

  let killed = false
  let closed = false
  const serverClosed = new Promise((resolve) => {
    server.on('close', () => {
      closed = true
      if (!killed) {
        console.log('Error: The registry server was killed!')
        process.exit(1)
      }
      resolve()
    })
  })
  server.on('error', (err) => {
    console.log(err)
  })
  global.killServer = async () => {
    killed = true
    if (closed) return
    if (server.pid != null) {
      try {
        await kill(server.pid)
      } catch (err) {
        if (!closed) throw err
      }
    } else {
      server.kill()
    }
    await Promise.race([
      serverClosed,
      scheduler.wait(10_000).then(() => {
        throw new Error('Timed out waiting for pnpr to exit')
      }),
    ])
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

function writeTestConfig (storage) {
  const source = path.join(REPO_ROOT, 'pnpr', 'crates', 'pnpr', 'config.yaml')
  const bundled = readFileSync(source, 'utf8')
  const configured = bundled.replace('max_users: -1', 'max_users: 100')
  if (configured === bundled) {
    throw new Error('pnpr test config could not enable test-only registration')
  }
  const target = path.join(storage, 'config.yaml')
  writeFileSync(target, configured)
  return target
}

/**
 * Build registry storage from the in-repo fixtures into `out` using the
 * `pnpr-prepare` binary (built from the `pnpr-fixtures` crate). The same
 * builder backs pacquet's in-process registry.
 */
function buildStorage (out) {
  const bin = resolvePnprPrepareBin()
  const result = spawnSync(bin, ['--packages', FIXTURE_PACKAGES, '--out', out], { stdio: 'inherit' })
  if (result.status !== 0) {
    throw new Error(
      `pnpr-prepare failed to build fixture storage (exit ${result.status ?? result.signal}).`
    )
  }
}

/**
 * Locate the `pnpr-prepare` binary. Lookup order:
 *
 * 1. `PNPR_PREPARE_BIN` env var (set by CI, which builds it from source).
 * 2. A locally-built `target/{release,debug}/pnpr-prepare`.
 */
function resolvePnprPrepareBin () {
  return resolveRustBin('pnpr-prepare', 'PNPR_PREPARE_BIN')
}

/**
 * Locate the `pnpr` server binary. Lookup order:
 *
 * 1. `PNPR_BIN` env var override.
 * 2. A locally-built `target/{release,debug}/pnpr`.
 *
 * There is no published-binary fallback on purpose: running these tests
 * already requires building `pnpr-prepare` from source (it has no npm
 * fallback either), so the toolchain to build `pnpr` is always present,
 * and a published `@pnpm/pnpr` could predate the server protocol the
 * tests exercise.
 */
function resolvePnprBin () {
  if (process.env.PNPR_BIN) {
    return process.env.PNPR_BIN
  }
  const localBin = findRustTargetBin('pnpr')
  if (localBin) return localBin
  throw new Error(
    'pnpr binary not found. Build it with `cargo build -p pnpr` or set PNPR_BIN to an absolute path.'
  )
}

function resolveRustBin (name, envVar) {
  if (process.env[envVar]) {
    return process.env[envVar]
  }
  const localBin = findRustTargetBin(name)
  if (localBin) return localBin
  throw new Error(
    `${name} binary not found. Build it with \`cargo build -p pnpr-fixtures --bin ${name}\` ` +
    `or set ${envVar} to an absolute path.`
  )
}

function findRustTargetBin (name) {
  const ext = process.platform === 'win32' ? '.exe' : ''
  for (const profile of ['release', 'debug']) {
    const candidate = path.join(REPO_ROOT, 'target', profile, `${name}${ext}`)
    if (existsSync(candidate)) return candidate
  }
  return undefined
}

const UNUSUAL_REGISTRY_STARTUP_THRESHOLD = 15 // seconds

async function waitForServerOnline () {
  const start = performance.now()

  for (const delay of exponentialBackoff()) {
    try {
      await fetch(`http://localhost:${process.env.PNPM_REGISTRY_MOCK_PORT}`, { method: 'HEAD' })

      const totalWait = (performance.now() - start) / 1000
      if (totalWait > UNUSUAL_REGISTRY_STARTUP_THRESHOLD) {
        console.warn(`pnpr required an unusually long amount of time to start: ${totalWait} seconds`)
      }

      return
    } catch (err) {
      // If pnpr hasn't begun listening yet, attempts to
      // connect to the unbound port should throw ECONNREFUSED. If a different
      // error is observed, throw an error.
      if (err?.cause?.code !== 'ECONNREFUSED') {
        throw new Error('Failed to bring pnpr online:', { cause: err })
      }

      await scheduler.wait(delay)
    }
  }

  const totalWait = (performance.now() - start) / 1000
  throw new Error(`pnpr did not come online after waiting ${totalWait} seconds`)
}

function *exponentialBackoff (attempts = 15, base = 1.5, initialWait = 100) {
  for (let i = 0; i < attempts; i++) {
    yield initialWait * Math.pow(base, i)
  }
}
