import http from 'node:http'
import https from 'node:https'
import { URL } from 'node:url'
import { gunzip } from 'node:zlib'

import type { Catalogs } from '@pnpm/catalogs.types'
import { hashObjectNullableWithPrefix } from '@pnpm/crypto.object-hasher'
import { PnpmError } from '@pnpm/error'
import { convertToLockfileObject } from '@pnpm/lockfile.fs'
import type { LockfileFile, LockfileObject } from '@pnpm/lockfile.types'
import type { PackageExtension, RegistryDeclaration, TrustPolicy } from '@pnpm/types'

import type { ResponseMetadata } from './protocol.js'

export interface PnprProject {
  /** Relative dir within the workspace (e.g. "." or "packages/foo") */
  dir: string
  name?: string
  version?: string
  dependencies?: Record<string, string>
  devDependencies?: Record<string, string>
  optionalDependencies?: Record<string, string>
}

export interface ResolveViaPnprServerOptions {
  /** URL of the pnpr server */
  registryUrl: string
  /** Project name to resolve (single project) */
  name?: string
  /** Project version to resolve (single project) */
  version?: string
  /** Dependencies to resolve (single project) */
  dependencies?: Record<string, string>
  /** Dev dependencies to resolve (single project) */
  devDependencies?: Record<string, string>
  /** Optional dependencies to resolve (single project) */
  optionalDependencies?: Record<string, string>
  /** Multiple projects in a workspace */
  projects?: PnprProject[]
  /**
   * The client's default registry. The server resolves against this
   * (and the prefix-addressed registries) rather than its own configuration.
   */
  registry?: string
  /**
   * The registries the client declares, keyed by URL, in the shape of the
   * `registries` setting. The default registry is not among them: it travels
   * as {@link ResolveViaPnprServerOptions.registry}.
   */
  registries?: Record<string, RegistryDeclaration>
  /**
   * `Authorization` for the pnpr server's own URL (`undefined` if none):
   * identifies the caller to pnpr's gate. The client never forwards its
   * own upstream registry credentials — pnpr selects upstream credentials
   * from its route policy, so none are placed in the request body.
   */
  authorization?: string
  /** Overrides */
  overrides?: Record<string, string>
  /** Patch selectors mapped to SHA-256 hashes; patch files stay client-side. */
  patchedDependencies?: Record<string, string>
  /** Manifest extensions applied during server-side resolution. */
  packageExtensions?: Record<string, PackageExtension>
  /** Allow configured patches that match no resolved package. */
  allowUnusedPatches?: boolean
  /**
   * Workspace catalogs (`catalog:` / `catalogs:` from `pnpm-workspace.yaml`),
   * keyed by catalog name with the default catalog under `default`. The
   * server resolves `catalog:` specifiers in dependencies and overrides
   * against these — it never reads the workspace manifest itself.
   */
  catalogs?: Catalogs
  /** Node.js version for resolution */
  nodeVersion?: string
  /**
   * The client's current values for the settings that shape the lockfile the
   * server resolves. Leaving one out is not the same as sending `false`: the
   * server then falls back to the input lockfile (on a frozen request) or to
   * its own default, which is what a client too old to send them gets
   * ([pnpm/pnpm#13389](https://github.com/pnpm/pnpm/issues/13389)).
   */
  autoInstallPeers?: boolean
  dedupePeers?: boolean
  excludeLinksFromLockfile?: boolean
  /**
   * The client's `resolutionMode`. The server picks versions the way the
   * client would, instead of falling back to its own default.
   */
  resolutionMode?: 'highest' | 'time-based' | 'lowest-direct'
  /**
   * The client's verification policy. The server is the only place these
   * run on the pnpr path — the client skips its own
   * `verifyLockfileResolutions` whenever a pnpr server is configured — so
   * every field has to travel with the request. Anything omitted is not
   * merely defaulted server-side: the server clears the field from its
   * config, which enforces a *stricter* policy than the user configured
   * (an omitted `minimumReleaseAgeExclude` re-applies the age gate to
   * packages the user opted out of).
   */
  minimumReleaseAge?: number
  minimumReleaseAgeExclude?: string[]
  minimumReleaseAgeIgnoreMissingTime?: boolean
  trustPolicy?: TrustPolicy
  trustPolicyExclude?: string[]
  trustPolicyIgnoreAfter?: number
  /**
   * The client's `trustLockfile` opt-out. When true the server skips the
   * input-lockfile verification gate but still reuses the lockfile for
   * resolution.
   */
  trustLockfile?: boolean
  /**
   * Resolution behavior — whether the server uses the lockfile as-is or
   * reuses its pins and resolves what changed. Distinct from the policy
   * fields above: neither affects whether the input lockfile is verified.
   */
  frozenLockfile?: boolean
  preferFrozenLockfile?: boolean
  /** Refresh registry artifacts while retaining every locked package version. */
  updatePatches?: boolean
  /**
   * Existing lockfile for incremental resolution, in the on-disk format
   * the wire protocol carries. The caller reads it with
   * `readWantedLockfileFile` so no in-memory→on-disk round-trip is needed.
   */
  lockfile?: LockfileFile
}

export interface ResolveViaPnprServerResult {
  lockfile: LockfileObject
  stats: ResponseMetadata['stats']
}

interface Violation { name: string, version: string, code: string, reason: string }

/**
 * One NDJSON frame from `POST /-/pnpr/v0/resolve`. `package` frames stream as
 * the server resolves; exactly one terminal frame (`done` / `error` /
 * `violations`) closes the response.
 */
type ResolveFrame =
  | { type: 'package', id: string, name: string, version: string, integrity: string, tarball: string }
  | { type: 'done', lockfile: LockfileFile, stats: ResponseMetadata['stats'] }
  | { type: 'error', message: string }
  | { type: 'violations', violations: Violation[] }

/**
 * Resolve a project against a pnpr server and return the resolved
 * lockfile.
 *
 * `POST /-/pnpr/v0/resolve` answers with an `application/x-ndjson` stream: one
 * `package` frame per resolved tarball as the server's tree walk yields
 * it, then exactly one terminal frame — `done` (full lockfile + stats),
 * `error`, or `violations`. pnpr serves no file content — the caller
 * fetches every tarball itself, in parallel, like a normal install
 * ([pnpm/pnpm#12230](https://github.com/pnpm/pnpm/issues/12230)).
 */
export async function resolveViaPnprServer (
  opts: ResolveViaPnprServerOptions
): Promise<ResolveViaPnprServerResult> {
  const projects = opts.projects ?? [{
    dir: '.',
    name: opts.name,
    version: opts.version,
    dependencies: opts.dependencies,
    devDependencies: opts.devDependencies,
    optionalDependencies: opts.optionalDependencies,
  }]

  const requestBody = JSON.stringify({
    projects,
    registry: opts.registry,
    registries: opts.registries,
    overrides: opts.overrides,
    patchedDependencies: opts.patchedDependencies,
    packageExtensions: opts.packageExtensions,
    allowUnusedPatches: opts.allowUnusedPatches,
    catalogs: opts.catalogs,
    nodeVersion: opts.nodeVersion ?? process.version.slice(1),
    autoInstallPeers: opts.autoInstallPeers,
    dedupePeers: opts.dedupePeers,
    excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
    os: process.platform,
    arch: process.arch,
    resolutionMode: opts.resolutionMode,
    minimumReleaseAge: opts.minimumReleaseAge,
    minimumReleaseAgeExclude: opts.minimumReleaseAgeExclude,
    minimumReleaseAgeIgnoreMissingTime: opts.minimumReleaseAgeIgnoreMissingTime,
    trustPolicy: opts.trustPolicy,
    trustPolicyExclude: opts.trustPolicyExclude,
    trustPolicyIgnoreAfter: opts.trustPolicyIgnoreAfter,
    trustLockfile: opts.trustLockfile,
    frozenLockfile: opts.frozenLockfile,
    preferFrozenLockfile: opts.preferFrozenLockfile,
    updatePatches: opts.updatePatches,
    // Sent as-is: `opts.lockfile` is already the on-disk format the wire
    // protocol carries (split `packages`/`snapshots`, `{ specifier, version }`
    // importer deps).
    lockfile: opts.lockfile,
  })

  const body = await postResolve(opts.registryUrl, requestBody, opts.authorization)

  const terminal = parseTerminalFrame(body.toString('utf-8'))

  if (terminal.type === 'error') {
    throw new Error(terminal.message)
  }
  if (terminal.type === 'violations') {
    const rendered = terminal.violations
      .map((violation) => `  ${violation.name}@${violation.version}: ${violation.reason}`)
      .join('\n')
    throw new Error(`pnpr server rejected the lockfile under the verification policy:\n${rendered}`)
  }

  assertTransformMetadata(terminal.lockfile, opts)

  return {
    // The server speaks the on-disk lockfile format; convert it to the
    // in-memory `LockfileObject` the rest of pnpm consumes.
    lockfile: convertToLockfileObject(terminal.lockfile),
    stats: terminal.stats,
  }
}

function assertTransformMetadata (
  lockfile: LockfileFile,
  opts: Pick<ResolveViaPnprServerOptions, 'patchedDependencies' | 'packageExtensions'>
): void {
  const expectedPatches = opts.patchedDependencies
  if (expectedPatches != null && Object.keys(expectedPatches).length > 0 && !equalStringRecords(lockfile.patchedDependencies, expectedPatches)) {
    throw new PnpmError('PNPR_TRANSFORM_METADATA_MISMATCH', 'pnpr server /-/pnpr/v0/resolve returned patchedDependencies that do not match the request; the server may not support project transforms')
  }

  const expectedExtensionsChecksum = hashObjectNullableWithPrefix(opts.packageExtensions)
  if (expectedExtensionsChecksum != null && lockfile.packageExtensionsChecksum !== expectedExtensionsChecksum) {
    throw new PnpmError('PNPR_TRANSFORM_METADATA_MISMATCH', 'pnpr server /-/pnpr/v0/resolve returned packageExtensionsChecksum that does not match the request; the server may not support project transforms')
  }
}

function equalStringRecords (
  actual: Record<string, string> | undefined,
  expected: Record<string, string>
): boolean {
  if (actual == null || Object.keys(actual).length !== Object.keys(expected).length) return false
  return Object.entries(expected).every(([key, value]) => actual[key] === value)
}

type TerminalFrame = Extract<ResolveFrame, { type: 'done' | 'error' | 'violations' }>

/**
 * Parse the NDJSON `/-/pnpr/v0/resolve` body and return its single terminal
 * frame. `package` frames are skipped — this client fetches tarballs the
 * normal way after resolution rather than overlapping fetch with the
 * stream. Throws on an unknown frame type (so a protocol mismatch fails
 * fast here rather than as a confusing lockfile error downstream) or if
 * the stream carries no terminal frame.
 */
function parseTerminalFrame (body: string): TerminalFrame {
  for (const line of body.split('\n')) {
    if (line.trim() === '') continue
    const frame = JSON.parse(line) as ResolveFrame
    if (frame.type === 'package') continue
    if (frame.type === 'done' || frame.type === 'error' || frame.type === 'violations') {
      return frame
    }
    throw new Error(`pnpr server /-/pnpr/v0/resolve stream emitted an unknown frame type: ${String((frame as { type: unknown }).type)}`)
  }
  throw new Error('pnpr server /-/pnpr/v0/resolve stream ended without a terminal frame')
}

const REQUEST_TIMEOUT = 600_000 // 10 minutes — server-side resolution can be slow on first run

/**
 * `POST /-/pnpr/v0/resolve` and return the full response body, decompressed.
 *
 * `urlPath` resolution normalizes the base to end with "/" so a path
 * prefix configured on the pnpr server URL (e.g. https://host/pnpr/) is
 * preserved.
 */
async function postResolve (registryUrl: string, body: string, authorization?: string): Promise<Buffer> {
  const base = registryUrl.endsWith('/') ? registryUrl : `${registryUrl}/`
  const url = new URL('-/pnpr/v0/resolve', base)
  const requestFn = url.protocol === 'https:' ? https.request : http.request

  const headers: http.OutgoingHttpHeaders = {
    'Content-Type': 'application/json',
    'Content-Length': Buffer.byteLength(body),
    'Accept-Encoding': 'gzip',
  }
  // Identify the caller to the pnpr server so private packages resolve
  // with the right credentials.
  if (authorization != null) {
    headers.Authorization = authorization
  }

  return new Promise<Buffer>((resolve, reject) => {
    const req = requestFn(url, {
      method: 'POST',
      timeout: REQUEST_TIMEOUT,
      headers,
    }, (res) => {
      const chunks: Buffer[] = []
      res.on('data', (chunk: Buffer) => chunks.push(chunk))
      res.on('end', () => {
        const raw = Buffer.concat(chunks)
        // The server gzips both the install body and its JSON error bodies
        // (e.g. a 401/403 access denial), so decompress *before* branching
        // on the status code — otherwise an error surfaces as binary
        // garbage instead of the server's message. Skip it only when the
        // HTTP stack already decompressed (no gzip magic bytes).
        const finish = (body: Buffer): void => {
          if (res.statusCode !== 200) {
            reject(new Error(`pnpr server responded with ${res.statusCode}: ${body.toString('utf-8')}`))
          } else {
            resolve(body)
          }
        }
        if (res.headers['content-encoding'] === 'gzip' || (raw[0] === 0x1f && raw[1] === 0x8b)) {
          gunzip(raw, (err, decompressed) => {
            if (err) reject(err)
            else finish(decompressed)
          })
        } else {
          finish(raw)
        }
      })
      res.on('error', reject)
    })

    req.on('timeout', () => {
      req.destroy(new Error(`pnpr server request timed out after ${REQUEST_TIMEOUT / 1000}s (${registryUrl})`))
    })
    req.on('error', (err: NodeJS.ErrnoException) => {
      if (err.code === 'ECONNREFUSED') {
        reject(new Error(`Could not connect to pnpr server at ${registryUrl}. Is the server running?`))
      } else {
        reject(err)
      }
    })
    req.write(body)
    req.end()
  })
}
