import { execFileSync } from 'node:child_process'
import path from 'node:path'

import type { AddUserResult } from '@pnpm/registry-access.client'

export const REGISTRY_MOCK_PORT = process.env.PNPM_REGISTRY_MOCK_PORT ?? '4873'

export const REGISTRY_MOCK_CREDENTIALS = {
  username: 'username',
  password: 'password',
}

export const REGISTRY_MOCK_URL = `http://localhost:${REGISTRY_MOCK_PORT}/npm/`

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

/**
 * A `minimumReleaseAge` (in minutes) under which `@pnpm.e2e/bravo@1.0.0`
 * (2022-04-01) is mature and `@pnpm.e2e/bravo-dep@1.1.0` (2022-05-01) is not.
 */
export function bravoMatureBravoDep110ImmatureMinimumReleaseAge (): number {
  return (Date.now() - new Date('2022-04-15T00:00:00.000Z').getTime()) / (60 * 1000)
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
    registryUrl: REGISTRY_MOCK_URL,
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
    registryUrl: REGISTRY_MOCK_URL,
    fetch: createFetchFromRegistry({}),
  })
}

/** pnpr's hosted namespace for npm registries without an org. */
const HOSTED_NPM_NAMESPACE = path.join('default', 'npm')

const integrities = new Map<string, string>()

/**
 * Reads a package version's tarball integrity from the registry mock's
 * packument, whether the version is a fixture or proxied from npm.
 *
 * Tests call this synchronously, so a child process fetches the packument.
 * A version's integrity never changes, so each one is fetched once.
 */
export function getIntegrity (pkgName: string, pkgVersion: string): string {
  const key = `${pkgName}@${pkgVersion}`
  let integrity = integrities.get(key)
  if (integrity == null) {
    integrity = fetchIntegrity(pkgName, pkgVersion)
    integrities.set(key, integrity)
  }
  return integrity
}

const FETCH_INTEGRITY = `
const [url, version] = process.argv.slice(1)
const response = await fetch(url, { headers: { accept: 'application/json' } })
if (!response.ok) throw new Error(\`GET \${url} answered \${response.status}\`)
const integrity = (await response.json()).versions?.[version]?.dist?.integrity
if (typeof integrity !== 'string') throw new Error(\`\${url} lists no integrity for \${version}\`)
process.stdout.write(integrity)
`

function fetchIntegrity (pkgName: string, pkgVersion: string): string {
  const url = `${REGISTRY_MOCK_URL}${pkgName.replaceAll('/', '%2f')}`
  return execFileSync(
    process.execPath,
    ['--input-type=module', '--eval', FETCH_INTEGRITY, url, pkgVersion],
    // A stalled lookup fails the test rather than holding the worker.
    { encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'], timeout: 30_000 }
  )
}

/**
 * Where the registry mock's storage keeps a fixture package's blob, such as
 * its tarball.
 */
export function hostedBlobPath (pkgName: string, fileName: string): string {
  const storage = process.env.PNPM_REGISTRY_MOCK_STORAGE
  if (!storage) {
    throw new Error('PNPM_REGISTRY_MOCK_STORAGE is not set; run the test under the with-registry jest preset.')
  }
  return path.join(packageDir(path.join(storage, HOSTED_NPM_NAMESPACE), pkgName), 'blobs', blobSegment(pkgName, fileName))
}

/**
 * Mirrors pnpr's storage layout (`pnpr/crates/storage/src/layout.rs`): a
 * package lives at `packages/@<scope>/<name>/`, or `packages/@/<name>/` when
 * it has no scope.
 */
function packageDir (namespace: string, pkgName: string): string {
  const separator = pkgName.indexOf('/')
  const segments = pkgName.startsWith('@') && separator > 1
    ? [`@${encodeSegment(pkgName.slice(1, separator))}`, encodeSegment(pkgName.slice(separator + 1))]
    : ['@', encodeSegment(pkgName)]
  return path.join(namespace, 'packages', ...segments)
}

/**
 * A tarball named `<basename>-<rest>` is stored as `<rest>`, and one not
 * named after its package keeps its name behind `@`.
 */
function blobSegment (pkgName: string, fileName: string): string {
  if (!fileName.endsWith('.tgz')) return encodeSegment(fileName)
  const prefix = `${pkgName.slice(pkgName.lastIndexOf('/') + 1)}-`
  const rest = fileName.startsWith(prefix) ? fileName.slice(prefix.length) : ''
  return rest !== '' && !rest.startsWith('.') ? encodeSegment(rest) : `@${encodeSegment(fileName)}`
}

/**
 * The storage's encoding of a name as one path segment: `[a-z0-9._-]` as is,
 * every other byte as `!` and two lowercase hex digits.
 */
function encodeSegment (text: string): string {
  let encoded = ''
  for (const byte of Buffer.from(text, 'utf8')) {
    const char = String.fromCharCode(byte)
    encoded += /[a-z0-9._-]/.test(char) ? char : `!${byte.toString(16).padStart(2, '0')}`
  }
  return encoded
}
