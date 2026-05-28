import { spawn, spawnSync } from 'node:child_process'
import { existsSync, mkdtempSync } from 'node:fs'
import { createRequire } from 'node:module'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { scheduler } from 'node:timers/promises'
import { promisify } from 'node:util'

import getPort from 'get-port'
import treeKill from 'tree-kill'

const kill = promisify(treeKill)
const require = createRequire(import.meta.url)

const REPO_ROOT = path.join(import.meta.dirname, '..', '..', '..')
const FIXTURE_PACKAGES = path.join(REPO_ROOT, 'registry', '.fixtures', 'packages')

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

  const bin = resolvePnpmRegistryBin()

  const server = spawn(
    bin,
    [
      '--listen', `127.0.0.1:${process.env.PNPM_REGISTRY_MOCK_PORT}`,
      '--storage', storage,
      '--upstream', process.env.PNPM_REGISTRY_MOCK_UPLINK ?? 'https://registry.npmjs.org',
      '--public-url', `http://localhost:${process.env.PNPM_REGISTRY_MOCK_PORT}`,
      // A one-year TTL so the fixture packuments (whose `time` is a fixed
      // placeholder) never look stale and never trigger a re-fetch to
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
 * Build registry storage from the in-repo fixtures into `out` using the
 * `pnpm-registry-prepare` binary (built from the `pnpm-registry-fixtures`
 * crate). The same builder backs pacquet's in-process registry.
 */
function buildStorage (out) {
  const bin = resolvePnpmRegistryPrepareBin()
  const result = spawnSync(bin, ['--packages', FIXTURE_PACKAGES, '--out', out], { stdio: 'inherit' })
  if (result.status !== 0) {
    throw new Error(
      `pnpm-registry-prepare failed to build fixture storage (exit ${result.status ?? result.signal}).`
    )
  }
}

/**
 * Locate the `pnpm-registry-prepare` binary. Lookup order:
 *
 * 1. `PNPM_REGISTRY_PREPARE_BIN` env var (set by CI, which builds it from source).
 * 2. A locally-built `target/{release,debug}/pnpm-registry-prepare`.
 */
function resolvePnpmRegistryPrepareBin () {
  return resolveRustBin('pnpm-registry-prepare', 'PNPM_REGISTRY_PREPARE_BIN')
}

/**
 * Locate the `pnpm-registry` server binary. Lookup order:
 *
 * 1. `PNPM_REGISTRY_BIN` env var override.
 * 2. A locally-built `target/{release,debug}/pnpm-registry`.
 * 3. The platform binary shipped as an optionalDependency of `@pnpm/pnpr`.
 */
function resolvePnpmRegistryBin () {
  if (process.env.PNPM_REGISTRY_BIN) {
    return process.env.PNPM_REGISTRY_BIN
  }
  const localBin = findRustTargetBin('pnpm-registry')
  if (localBin) return localBin

  const ext = process.platform === 'win32' ? '.exe' : ''
  const platformPkg = `@pnpm/pnpr.${process.platform}-${process.arch}`
  try {
    const wrapperRequire = createRequire(require.resolve('@pnpm/pnpr/bin/pnpr'))
    return wrapperRequire.resolve(`${platformPkg}/pnpr${ext}`)
  } catch {
    throw new Error(
      'pnpm-registry binary not found. Build it with `cargo build -p pnpm-registry`, ' +
      `set PNPM_REGISTRY_BIN, or install ${platformPkg} (an optionalDependency of @pnpm/pnpr).`
    )
  }
}

function resolveRustBin (name, envVar) {
  if (process.env[envVar]) {
    return process.env[envVar]
  }
  const localBin = findRustTargetBin(name)
  if (localBin) return localBin
  throw new Error(
    `${name} binary not found. Build it with \`cargo build -p pnpm-registry-fixtures --bin ${name}\` ` +
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
