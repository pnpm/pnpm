import fs from 'fs'
import path from 'path'

import { createFetchFromRegistry } from '@pnpm/network.fetch'
import { addUser as setUser, type AddUserResult, setDistTag } from '@pnpm/registry-access.client'

export const REGISTRY_MOCK_PORT = process.env.PNPM_REGISTRY_MOCK_PORT ?? '4873'

export const REGISTRY_MOCK_CREDENTIALS = {
  username: 'username',
  password: 'password',
}

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}/`

const AUTH_HEADER = `Basic ${Buffer.from(
  `${REGISTRY_MOCK_CREDENTIALS.username}:${REGISTRY_MOCK_CREDENTIALS.password}`
).toString('base64')}`

const fetchFromRegistry = createFetchFromRegistry({})

export interface AddDistTagOptions {
  package: string
  version: string
  distTag: string
}

export async function addDistTag (opts: AddDistTagOptions): Promise<void> {
  await setDistTag({
    packageName: opts.package,
    version: opts.version,
    distTag: opts.distTag,
    registryUrl: REGISTRY_URL,
    authHeader: AUTH_HEADER,
    fetchFromRegistry,
  })
}

export interface AddUserOptions {
  username: string
  password: string
  email: string
}

export async function addUser (opts: AddUserOptions): Promise<AddUserResult> {
  return setUser({
    username: opts.username,
    password: opts.password,
    email: opts.email,
    registryUrl: REGISTRY_URL,
    fetch: fetchFromRegistry,
  })
}

/**
 * Reads a package version's tarball integrity from the registry storage that
 * the test harness built from the fixtures. The storage path is published as
 * `PNPM_REGISTRY_MOCK_STORAGE` by the with-registry jest globalSetup.
 *
 * Uplinked packages (proxied from the upstream registry) are written to
 * storage lazily on first request, so the read is retried briefly while the
 * packument is still being written.
 */
export function getIntegrity (pkgName: string, pkgVersion: string): string {
  const storage = process.env.PNPM_REGISTRY_MOCK_STORAGE
  if (!storage) {
    throw new Error(
      'PNPM_REGISTRY_MOCK_STORAGE is not set — the registry mock storage path is unknown. ' +
      'Tests that call getIntegrity must run under the with-registry jest preset.'
    )
  }
  const filePath = path.join(storage, pkgName, 'package.json')
  const maxRetries = 4
  let delay = 200 // milliseconds
  let content: { versions: Record<string, { dist: { integrity: string } }> } | undefined
  for (let attempt = 1; attempt <= maxRetries; attempt++) {
    try {
      content = JSON.parse(fs.readFileSync(filePath, 'utf8'))
      break
    } catch (err: unknown) {
      if (attempt === maxRetries || !isTransientReadError(err)) {
        throw err
      }
      Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, delay)
      delay *= 2
    }
  }
  if (!content) {
    throw new Error(`Failed to read package.json for ${pkgName}@${pkgVersion} after ${maxRetries} attempts`)
  }
  return content.versions[pkgVersion].dist.integrity
}

function isTransientReadError (err: unknown): boolean {
  if (!err || typeof err !== 'object') return false
  if (err instanceof SyntaxError && err.message.endsWith('Unexpected end of JSON input')) return true
  return (err as NodeJS.ErrnoException).code === 'ENOENT'
}
