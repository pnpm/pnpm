import fs from 'node:fs'
import path from 'node:path'

import type { AddUserResult } from '@pnpm/registry-access.client'

export const REGISTRY_MOCK_PORT = process.env.PNPM_REGISTRY_MOCK_PORT ?? '4873'

export const REGISTRY_MOCK_CREDENTIALS = {
  username: 'username',
  password: 'password',
}

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}/`

const AUTH_HEADER = `Basic ${Buffer.from(
  `${REGISTRY_MOCK_CREDENTIALS.username}:${REGISTRY_MOCK_CREDENTIALS.password}`
).toString('base64')}`

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
    authHeader: AUTH_HEADER,
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
 * packages are proxied into `<storage>/.pnpr-cache/<pkg>/package.json`, written
 * lazily on first request — so both are tried and the read is retried while the
 * cache write is still in flight.
 */
export function getIntegrity (pkgName: string, pkgVersion: string): string {
  const storage = process.env.PNPM_REGISTRY_MOCK_STORAGE
  if (!storage) {
    throw new Error(
      'PNPM_REGISTRY_MOCK_STORAGE is not set — the registry mock storage path is unknown. ' +
      'Tests that call getIntegrity must run under the with-registry jest preset.'
    )
  }
  const candidatePaths = [
    path.join(storage, pkgName, 'package.json'),
    path.join(storage, PROXY_CACHE_DIR, pkgName, 'package.json'),
  ]
  const maxRetries = 4
  let delay = 200 // milliseconds
  for (let attempt = 1; attempt <= maxRetries; attempt++) {
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
