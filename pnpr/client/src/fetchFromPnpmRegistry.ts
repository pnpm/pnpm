import http from 'node:http'
import https from 'node:https'
import { URL } from 'node:url'
import { gunzip } from 'node:zlib'

import { convertToLockfileObject } from '@pnpm/lockfile.fs'
import type { LockfileFile, LockfileObject } from '@pnpm/lockfile.types'
import { StoreIndex } from '@pnpm/store.index'
import { writeCafsFiles } from '@pnpm/worker'

import type { ResponseMetadata } from './protocol.js'

export interface PnprProject {
  /** Relative dir within the workspace (e.g. "." or "packages/foo") */
  dir: string
  dependencies?: Record<string, string>
  devDependencies?: Record<string, string>
  optionalDependencies?: Record<string, string>
}

export interface FetchFromPnpmRegistryOptions {
  /** URL of the pnpr server */
  registryUrl: string
  /** Client's store directory */
  storeDir: string
  /** Client's store index */
  storeIndex: StoreIndex
  /** Dependencies to resolve (single project) */
  dependencies?: Record<string, string>
  /** Dev dependencies to resolve (single project) */
  devDependencies?: Record<string, string>
  /** Optional dependencies to resolve (single project) */
  optionalDependencies?: Record<string, string>
  /** Multiple projects in a workspace */
  projects?: PnprProject[]
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
  /**
   * `--lockfile-only`: resolve and return only the lockfile — fetch no
   * files into the local store. Forwarded to the server (which skips the
   * file diff); the client ignores the (empty) file payload so the store
   * stays untouched. Mirrors pnpm's resolve + write, fetch nothing, link
   * nothing. See https://github.com/pnpm/pnpm/issues/12146.
   */
  lockfileOnly?: boolean
}

export interface FetchFromPnpmRegistryResult {
  lockfile: LockfileObject
  stats: ResponseMetadata['stats']
  /** Promise that resolves when all file downloads are written to CAFS */
  fileDownloads: Promise<void>
  /** Pre-packed store index entries to write to SQLite */
  indexEntries: Array<{ key: string, buffer: Uint8Array }>
}

interface InstallResponseHeader {
  lockfile: LockfileFile
  stats: ResponseMetadata['stats']
  indexEntries?: Array<{ key: string, b64: string }>
  violations?: Array<{ name: string, version: string, code: string, reason: string }>
}

/**
 * Fetch resolved dependencies from a pnpr server in a single round trip.
 *
 * `POST /v1/install` (with `inlineFiles`) answers with one gzipped binary
 * body: a length-prefixed JSON header (lockfile, stats, store-index
 * entries, or verification violations) followed by the missing files'
 * contents as binary frames. We parse the header here and hand the file
 * frames to a worker that writes them straight into the CAFS.
 */
export async function fetchFromPnpmRegistry (
  opts: FetchFromPnpmRegistryOptions
): Promise<FetchFromPnpmRegistryResult> {
  const storeIntegrities = readStoreIntegrities(opts.storeIndex)

  const projects = opts.projects ?? [{
    dir: '.',
    dependencies: opts.dependencies,
    devDependencies: opts.devDependencies,
    optionalDependencies: opts.optionalDependencies,
  }]

  const requestBody = JSON.stringify({
    projects,
    overrides: opts.overrides,
    nodeVersion: opts.nodeVersion ?? process.version.slice(1),
    os: process.platform,
    arch: process.arch,
    minimumReleaseAge: opts.minimumReleaseAge,
    // Sent as-is: `opts.lockfile` is already the on-disk format the wire
    // protocol carries (split `packages`/`snapshots`, `{ specifier, version }`
    // importer deps).
    lockfile: opts.lockfile,
    lockfileOnly: opts.lockfileOnly,
    storeIntegrities,
    inlineFiles: true,
  })

  const body = await postInstall(opts.registryUrl, requestBody)

  // The combined response is `[u32 header length][header JSON][file frames]`.
  if (body.length < 4) {
    throw new Error('pnpr server returned a truncated /v1/install response')
  }
  const headerLength = body.readUInt32BE(0)
  const header = JSON.parse(body.subarray(4, 4 + headerLength).toString('utf-8')) as InstallResponseHeader

  if (header.violations != null && header.violations.length > 0) {
    const rendered = header.violations
      .map((violation) => `  ${violation.name}@${violation.version}: ${violation.reason}`)
      .join('\n')
    throw new Error(`pnpr server rejected the lockfile under the verification policy:\n${rendered}`)
  }

  const indexEntries = (header.indexEntries ?? []).map(({ key, b64 }) => ({
    key,
    buffer: new Uint8Array(Buffer.from(b64, 'base64')),
  }))

  // `--lockfile-only` fetches nothing: there are no file frames to write
  // (the server sends only the end-of-stream marker), so leave the store
  // untouched.
  const fileDownloads = opts.lockfileOnly
    ? Promise.resolve()
    : writeCafsFiles({
      storeDir: opts.storeDir,
      payload: body.subarray(4 + headerLength),
    }).then(() => {})

  return {
    // The server speaks the on-disk lockfile format; convert it to the
    // in-memory `LockfileObject` the rest of pnpm consumes.
    lockfile: convertToLockfileObject(header.lockfile),
    stats: header.stats,
    fileDownloads,
    indexEntries,
  }
}

function readStoreIntegrities (storeIndex: StoreIndex): string[] {
  const seen = new Set<string>()
  for (const key of storeIndex.keys()) {
    const tabIdx = key.indexOf('\t')
    if (tabIdx === -1) continue
    const integrity = key.slice(0, tabIdx)
    // StoreIndex also stores non-integrity keys (e.g. git-hosted entries
    // keyed by URL). Filter to actual SRI hashes — sending those over to
    // the pnpr server would just bloat the request without ever matching.
    if (!isIntegrityLike(integrity)) continue
    seen.add(integrity)
  }
  return [...seen]
}

function isIntegrityLike (value: string): boolean {
  return value.startsWith('sha512-') ||
    value.startsWith('sha256-') ||
    value.startsWith('sha1-')
}

const REQUEST_TIMEOUT = 600_000 // 10 minutes — server-side resolution can be slow on first run

/**
 * `POST /v1/install` and return the full response body, decompressed.
 *
 * `urlPath` resolution normalizes the base to end with "/" so a path
 * prefix configured on the pnpr server URL (e.g. https://host/pnpr/) is
 * preserved.
 */
async function postInstall (registryUrl: string, body: string): Promise<Buffer> {
  const base = registryUrl.endsWith('/') ? registryUrl : `${registryUrl}/`
  const url = new URL('v1/install', base)
  const requestFn = url.protocol === 'https:' ? https.request : http.request

  return new Promise<Buffer>((resolve, reject) => {
    const req = requestFn(url, {
      method: 'POST',
      timeout: REQUEST_TIMEOUT,
      headers: {
        'Content-Type': 'application/json',
        'Content-Length': Buffer.byteLength(body),
        'Accept-Encoding': 'gzip',
      },
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

export function writeRawIndexEntries (
  indexEntries: Array<{ key: string, buffer: Uint8Array }>,
  storeIndex: StoreIndex
): void {
  const writes = indexEntries.filter(({ key }) => !storeIndex.has(key))
  if (writes.length > 0) {
    storeIndex.setRawMany(writes)
  }
}
