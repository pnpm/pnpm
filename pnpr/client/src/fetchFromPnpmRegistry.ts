import http from 'node:http'
import https from 'node:https'
import { URL } from 'node:url'

import { convertToLockfileObject } from '@pnpm/lockfile.fs'
import type { LockfileFile, LockfileObject } from '@pnpm/lockfile.types'
import { StoreIndex } from '@pnpm/store.index'
import { fetchAndWriteCafsFiles } from '@pnpm/worker'

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
   * file diff when it understands the flag); the client also ignores any
   * `D`/`I` lines so the store stays untouched even against an older
   * server. Mirrors pnpm's resolve + write, fetch nothing, link nothing.
   * See https://github.com/pnpm/pnpm/issues/12146.
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

/**
 * Fetch resolved dependencies from a pnpr server.
 *
 * The response is a streaming NDJSON where each line is one message:
 *   - `D\t{digest}\t{size}\t{executable}\n` — file digest (streamed as packages resolve)
 *   - `L\t{json}\n` — final lockfile (on-disk format) + stats (after resolution)
 *   - `I\t{key}\t{base64}\n` — pre-packed msgpack index entry
 *
 * As digest lines arrive, we batch them and dispatch workers to /v1/files.
 * File downloads happen IN PARALLEL with server-side resolution.
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
  })

  const indexEntries: Array<{ key: string, buffer: Uint8Array }> = []
  const workerPromises: Array<Promise<number>> = []
  let currentBatch: Array<{ digest: string, size: number, executable: boolean }> = []

  const dispatchBatch = () => {
    if (currentBatch.length === 0) return
    const digests = currentBatch
    currentBatch = []
    workerPromises.push(fetchAndWriteCafsFiles({
      registryUrl: opts.registryUrl,
      storeDir: opts.storeDir,
      digests,
    }))
  }

  // Returns as soon as the lockfile arrives — the stream continues
  // in the background, dispatching more file download workers.
  // fileDownloads covers ALL workers (past and future).
  return new Promise<FetchFromPnpmRegistryResult>((resolve, reject) => {
    let resolved = false
    let serverError: Error | undefined
    const handleLine = (line: string) => {
      if (line.length === 0) return
      const tabIdx = line.indexOf('\t')
      const type = line.charAt(0)
      if (type === 'D') {
        // `--lockfile-only` fetches nothing — ignore any file digests an
        // older server still streams rather than writing them to the store.
        if (opts.lockfileOnly) return
        const parts = line.split('\t')
        currentBatch.push({
          digest: parts[1],
          size: parseInt(parts[2], 10),
          executable: parts[3] === '1',
        })
        if (currentBatch.length >= FILES_PER_WORKER) {
          dispatchBatch()
        }
      } else if (type === 'L') {
        const payload = JSON.parse(line.substring(tabIdx + 1)) as {
          lockfile: LockfileFile
          stats: ResponseMetadata['stats']
        }
        dispatchBatch()
        resolved = true
        // Resolve immediately — the caller can start headless install
        // while the stream continues dispatching remaining D/I lines.
        resolve({
          // The server speaks the on-disk lockfile format; convert it to the
          // in-memory `LockfileObject` the rest of pnpm consumes.
          lockfile: convertToLockfileObject(payload.lockfile),
          stats: payload.stats,
          fileDownloads: streamComplete.then(() =>
            Promise.all(workerPromises)
          ).then(() => {}),
          indexEntries,
        })
      } else if (type === 'I') {
        // `--lockfile-only` writes no store-index entries — ignore any an
        // older server still streams.
        if (opts.lockfileOnly) return
        // Format: I\t{integrity}\t{pkgId}\t{base64}
        // Key is "{integrity}\t{pkgId}" — everything between first and last tab
        const rest = line.substring(tabIdx + 1)
        const lastTab = rest.lastIndexOf('\t')
        const key = rest.substring(0, lastTab)
        const buffer = new Uint8Array(Buffer.from(rest.substring(lastTab + 1), 'base64'))
        indexEntries.push({ key, buffer })
      } else if (type === 'E') {
        // Server emitted a structured error after headers were sent.
        // Record it so stream `end` / `catch` can reject with the payload.
        let message = 'pnpr server error'
        try {
          const payload = JSON.parse(line.substring(tabIdx + 1)) as { error?: string }
          if (payload?.error) message = payload.error
        } catch {
          // Fall back to the raw payload if it isn't JSON.
          message = line.substring(tabIdx + 1) || message
        }
        serverError = new Error(message)
      }
    }

    const streamComplete = streamNdjsonRequest(
      opts.registryUrl, 'v1/install', requestBody, handleLine
    )

    streamComplete.then(() => {
      if (serverError) {
        reject(serverError)
      } else if (!resolved) {
        reject(new Error('pnpr server closed the stream without emitting a lockfile'))
      }
    }, reject)
  })
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

const FILES_PER_WORKER = 4000
const REQUEST_TIMEOUT = 600_000 // 10 minutes — server-side resolution can be slow on first run

/**
 * Stream an NDJSON response, calling `onLine` for each complete line as
 * it arrives. Chunks are buffered until a newline is seen.
 */
async function streamNdjsonRequest (
  registryUrl: string,
  urlPath: string,
  body: string,
  onLine: (line: string) => void
): Promise<void> {
  // `urlPath` is expected to be relative (e.g. "v1/install"). We normalize
  // the base to end with "/" so `new URL(rel, base)` preserves any path
  // prefix configured on the pnpr server URL (e.g. https://host/pnpr/).
  const base = registryUrl.endsWith('/') ? registryUrl : `${registryUrl}/`
  const url = new URL(urlPath, base)
  const isHttps = url.protocol === 'https:'
  const requestFn = isHttps ? https.request : http.request

  return new Promise<void>((resolve, reject) => {
    const req = requestFn(url, {
      method: 'POST',
      timeout: REQUEST_TIMEOUT,
      headers: {
        'Content-Type': 'application/json',
        'Content-Length': Buffer.byteLength(body),
        'Accept': 'application/x-ndjson',
      },
    }, (res) => {
      if (res.statusCode !== 200) {
        const chunks: Buffer[] = []
        res.on('data', (chunk: Buffer) => chunks.push(chunk))
        res.on('end', () => {
          reject(new Error(`pnpr server responded with ${res.statusCode}: ${Buffer.concat(chunks).toString('utf-8')}`))
        })
        return
      }

      let leftover = ''
      res.setEncoding('utf-8')
      res.on('data', (chunk: string) => {
        const data = leftover + chunk
        const lines = data.split('\n')
        leftover = lines.pop() ?? ''
        for (const line of lines) {
          onLine(line)
        }
      })
      res.on('end', () => {
        if (leftover) onLine(leftover)
        resolve()
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


