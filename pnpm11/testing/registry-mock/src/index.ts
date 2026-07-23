import fs from 'node:fs'
import path from 'node:path'

import type { AddUserResult } from '@pnpm/registry-access.client'

export const REGISTRY_MOCK_PORT = process.env.PNPM_REGISTRY_MOCK_PORT ?? '4873'

export const REGISTRY_MOCK_CREDENTIALS = {
  username: 'username',
  password: 'password',
}

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}/`

/**
 * The bearer token the with-registry jest globalSetup mints for the test user
 * and publishes as `REGISTRY_MOCK_TOKEN`.
 *
 * pnpr honors only `Authorization: Bearer` credentials on requests; HTTP Basic
 * (`_auth`) resolves to anonymous. So any test that authenticates to the mock
 * registry (publishing, setting a dist-tag, etc. on a `publish: $authenticated`
 * package) must send this token. Throws if the with-registry preset hasn't run.
 */
export function getRegistryMockToken (): string {
  const token = process.env.REGISTRY_MOCK_TOKEN
  if (!token) {
    throw new Error(
      'REGISTRY_MOCK_TOKEN is not set — the registry mock auth token is unknown. ' +
      'Tests that authenticate to the registry mock must run under the with-registry jest preset.'
    )
  }
  return token
}

/**
 * A `minimumReleaseAge` (in minutes) under which `@pnpm.e2e/bravo-dep`'s
 * `latest` tag (1.1.0) is the only immature version.
 *
 * The mock registry publishes `@pnpm.e2e/bravo-dep` at 1.0.0 (2022-02-01),
 * 1.0.1 (2022-02-22), and 1.1.0 (2022-05-01) — see `version_publish_time` in
 * `pnpr/crates/pnpr-fixtures/src/lib.rs` — so a cutoff anchored at 2022-03-01
 * lands between the last two.
 */
export function bravoDepMatureUpTo101MinimumReleaseAge (): number {
  return (Date.now() - new Date('2022-03-01T00:00:00.000Z').getTime()) / (60 * 1000)
}

export interface AddDistTagOptions {
  package: string
  version: string
  distTag: string
}

// `@pnpm/network.fetch` and `@pnpm/registry-access.client` are imported lazily
// because they transitively load `@pnpm/core-loggers`/`@pnpm/logger`. This module
// is imported (for the constants/getIntegrity) by test helpers that other tests
// pull in statically before calling `jest.unstable_mockModule('@pnpm/logger')`,
// so eagerly loading the logger here would defeat that mock.
export async function addDistTag (opts: AddDistTagOptions): Promise<void> {
  const { createFetchFromRegistry } = await import('@pnpm/network.fetch')
  const { setDistTag } = await import('@pnpm/registry-access.client')
  await setDistTag({
    packageName: opts.package,
    version: opts.version,
    distTag: opts.distTag,
    registryUrl: REGISTRY_URL,
    authHeader: `Bearer ${getRegistryMockToken()}`,
    fetchFromRegistry: createFetchFromRegistry({}),
  })
}

export interface AddUserOptions {
  username: string
  password: string
  email: string
}

export async function addUser (opts: AddUserOptions): Promise<AddUserResult> {
  const { createFetchFromRegistry } = await import('@pnpm/network.fetch')
  const { addUser: setUser } = await import('@pnpm/registry-access.client')
  return setUser({
    username: opts.username,
    password: opts.password,
    email: opts.email,
    registryUrl: REGISTRY_URL,
    fetch: createFetchFromRegistry({}),
  })
}

// pnpr keeps proxied upstream packages in a cache mirror separate from hosted
// (fixture) packages — a `.pnpr-cache` subdirectory of the storage root.
const PROXY_CACHE_DIR = '.pnpr-cache'

type Packument = { versions: Record<string, { dist: { integrity: string } }> }

/**
 * Reads a package version's tarball integrity from the registry storage that
 * the test harness built from the fixtures. The storage path is published as
 * `PNPM_REGISTRY_MOCK_STORAGE` by the with-registry jest globalSetup.
 *
 * Hosted (fixture) packuments live at `<storage>/<pkg>/package.json`; upstream
 * packages are proxied into a per-upstream namespace of the cache mirror
 * (`<storage>/.pnpr-cache/~public/<digest>/<pkg>/package.json`), written
 * lazily on first request — so every namespace is tried and the read is
 * retried while the cache write is still in flight.
 */
export function getIntegrity (pkgName: string, pkgVersion: string): string {
  const storage = process.env.PNPM_REGISTRY_MOCK_STORAGE
  if (!storage) {
    throw new Error(
      'PNPM_REGISTRY_MOCK_STORAGE is not set — the registry mock storage path is unknown. ' +
      'Tests that call getIntegrity must run under the with-registry jest preset.'
    )
  }
  const maxRetries = 4
  let delay = 200 // milliseconds
  for (let attempt = 1; attempt <= maxRetries; attempt++) {
    // Rebuilt on every attempt: the proxy-cache namespace directory itself is
    // created lazily along with the first cached packument, so it may not
    // exist yet on the first attempt.
    const candidatePaths = [
      path.join(storage, pkgName, 'package.json'),
      ...listPublicProxyNamespaces(path.join(storage, PROXY_CACHE_DIR, '~public'))
        .map((namespace) => path.join(namespace, pkgName, 'package.json')),
    ]
    const content = readPackument(candidatePaths)
    if (content) {
      return content.versions[pkgVersion].dist.integrity
    }
    if (attempt === maxRetries) break
    Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, delay)
    delay *= 2
  }
  throw new Error(`Failed to read package.json for ${pkgName}@${pkgVersion} after ${maxRetries} attempts`)
}

// The public proxy cache is namespaced per upstream mount by a digest of its
// name and URL, which the test side cannot compute — so enumerate whatever
// namespaces exist (for the registry mock, at most one: npmjs).
function listPublicProxyNamespaces (publicCacheDir: string): string[] {
  let entries: fs.Dirent[]
  try {
    entries = fs.readdirSync(publicCacheDir, { withFileTypes: true })
  } catch (err: unknown) {
    if ((err as NodeJS.ErrnoException).code === 'ENOENT') return []
    throw err
  }
  return entries
    .filter((entry) => entry.isDirectory())
    .map((entry) => path.join(publicCacheDir, entry.name))
}

// Returns the first readable packument among the candidates, or `undefined` when
// none is ready yet (missing file or partial write) so the caller retries.
// Re-throws any other read/parse error so genuine failures surface immediately.
function readPackument (candidatePaths: string[]): Packument | undefined {
  for (const filePath of candidatePaths) {
    let raw: string
    try {
      raw = fs.readFileSync(filePath, 'utf8')
    } catch (err: unknown) {
      if ((err as NodeJS.ErrnoException).code === 'ENOENT') continue
      throw err
    }
    try {
      return JSON.parse(raw) as Packument
    } catch (err: unknown) {
      if (err instanceof SyntaxError && err.message.endsWith('Unexpected end of JSON input')) {
        return undefined
      }
      throw err
    }
  }
  return undefined
}
