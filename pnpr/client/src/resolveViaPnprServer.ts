import http from 'node:http'
import https from 'node:https'
import { URL } from 'node:url'
import { gunzip } from 'node:zlib'

import { convertToLockfileObject } from '@pnpm/lockfile.fs'
import type { LockfileFile, LockfileObject } from '@pnpm/lockfile.types'

import type { ResponseMetadata } from './protocol.js'

export type AuthHeadersByScope = Record<string, Record<string, string>>

export interface PnprProject {
  /** Relative dir within the workspace (e.g. "." or "packages/foo") */
  dir: string
  dependencies?: Record<string, string>
  devDependencies?: Record<string, string>
  optionalDependencies?: Record<string, string>
}

export interface ResolveViaPnprServerOptions {
  /** URL of the pnpr server */
  registryUrl: string
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
   * (and `namedRegistries`) rather than its own configuration.
   */
  registry?: string
  /** The client's named-registry aliases (`namedRegistries`). */
  namedRegistries?: Record<string, string>
  /**
   * The caller's forwarded upstream credentials, keyed by nerf-darted
   * registry URI and package scope, so the server resolves private
   * content as the caller. The `@` scope stores registry-wide auth.
   * Distinct from `authorization` (pnpr identity).
   */
  authHeaders?: AuthHeadersByScope
  /**
   * `Authorization` for the pnpr server's own URL (`undefined` if none):
   * identifies the caller to pnpr's gate.
   */
  authorization?: string
  /** Overrides */
  overrides?: Record<string, string>
  /** Node.js version for resolution */
  nodeVersion?: string
  /** Minimum release age in minutes */
  minimumReleaseAge?: number
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
    dependencies: opts.dependencies,
    devDependencies: opts.devDependencies,
    optionalDependencies: opts.optionalDependencies,
  }]

  const requestBody = JSON.stringify({
    projects,
    registry: opts.registry,
    namedRegistries: opts.namedRegistries,
    authHeaders: opts.authHeaders,
    overrides: opts.overrides,
    nodeVersion: opts.nodeVersion ?? process.version.slice(1),
    os: process.platform,
    arch: process.arch,
    minimumReleaseAge: opts.minimumReleaseAge,
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

  return {
    // The server speaks the on-disk lockfile format; convert it to the
    // in-memory `LockfileObject` the rest of pnpm consumes.
    lockfile: convertToLockfileObject(terminal.lockfile),
    stats: terminal.stats,
  }
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
