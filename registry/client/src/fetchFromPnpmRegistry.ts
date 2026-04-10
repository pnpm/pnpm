import http from 'node:http'
import https from 'node:https'
import { URL } from 'node:url'

import type { LockfileObject } from '@pnpm/lockfile.types'
import { StoreIndex } from '@pnpm/store.index'
import { fetchAndWriteCafsFiles } from '@pnpm/worker'

import type { ResponseMetadata } from './protocol.js'

export interface FetchFromPnpmRegistryOptions {
  /** URL of the pnpm registry server */
  registryUrl: string
  /** Client's store directory */
  storeDir: string
  /** Client's store index */
  storeIndex: StoreIndex
  /** Dependencies to resolve */
  dependencies?: Record<string, string>
  /** Dev dependencies to resolve */
  devDependencies?: Record<string, string>
  /** Overrides */
  overrides?: Record<string, string>
  /** Node.js version for resolution */
  nodeVersion?: string
  /** Existing lockfile for incremental resolution */
  lockfile?: LockfileObject
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
 * Fetch resolved dependencies from a pnpm registry server.
 *
 * The response is a streaming NDJSON where each line is one message:
 *   - `D\t{digest}\t{size}\t{executable}\n` — file digest (streamed as packages resolve)
 *   - `L\t{json}\n` — final lockfile + stats (after resolution)
 *   - `I\t{key}\t{base64}\n` — pre-packed msgpack index entry
 *
 * As digest lines arrive, we batch them and dispatch workers to /v1/files.
 * File downloads happen IN PARALLEL with server-side resolution.
 */
export async function fetchFromPnpmRegistry (
  opts: FetchFromPnpmRegistryOptions
): Promise<FetchFromPnpmRegistryResult> {
  const storeIntegrities = readStoreIntegrities(opts.storeIndex)

  const requestBody = JSON.stringify({
    dependencies: opts.dependencies,
    devDependencies: opts.devDependencies,
    overrides: opts.overrides,
    nodeVersion: opts.nodeVersion ?? process.version.slice(1),
    os: process.platform,
    arch: process.arch,
    lockfile: opts.lockfile,
    storeIntegrities,
  })

  const indexEntries: Array<{ key: string, buffer: Uint8Array }> = []
  const workerPromises: Array<Promise<number>> = []
  let currentBatch: Array<{ digest: string, size: number, executable: boolean }> = []
  let lockfile: LockfileObject | undefined
  let stats: ResponseMetadata['stats'] | undefined

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

  const handleLine = (line: string) => {
    if (line.length === 0) return
    const tabIdx = line.indexOf('\t')
    const type = line.charAt(0)
    if (type === 'D') {
      // D\tdigest\tsize\texecutable
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
      // L\t<json>
      const payload = JSON.parse(line.substring(tabIdx + 1)) as {
        lockfile: LockfileObject
        stats: ResponseMetadata['stats']
      }
      lockfile = payload.lockfile
      stats = payload.stats
      // Dispatch any remaining digests before the lockfile is returned
      dispatchBatch()
    } else if (type === 'I') {
      // I\tkey\tbase64msgpack
      const rest = line.substring(tabIdx + 1)
      const secondTab = rest.indexOf('\t')
      const key = rest.substring(0, secondTab)
      const buffer = new Uint8Array(Buffer.from(rest.substring(secondTab + 1), 'base64'))
      indexEntries.push({ key, buffer })
    }
  }

  await streamNdjsonRequest(opts.registryUrl, '/v1/install', requestBody, handleLine)

  if (!lockfile || !stats) {
    throw new Error('pnpm-registry response missing lockfile')
  }

  const fileDownloads = Promise.all(workerPromises).then(() => {})

  return { lockfile, stats, fileDownloads, indexEntries }
}

function readStoreIntegrities (storeIndex: StoreIndex): string[] {
  const seen = new Set<string>()
  for (const key of storeIndex.keys()) {
    const tabIdx = key.indexOf('\t')
    if (tabIdx === -1) continue
    seen.add(key.slice(0, tabIdx))
  }
  return [...seen]
}

const FILES_PER_WORKER = 4000
const REQUEST_TIMEOUT = 120_000

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
  const url = new URL(urlPath, registryUrl)
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
          reject(new Error(`pnpm-registry responded with ${res.statusCode}: ${Buffer.concat(chunks).toString('utf-8')}`))
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
      req.destroy(new Error(`pnpm-registry request timed out after ${REQUEST_TIMEOUT / 1000}s (${registryUrl})`))
    })
    req.on('error', (err: NodeJS.ErrnoException) => {
      if (err.code === 'ECONNREFUSED') {
        reject(new Error(`Could not connect to pnpm-registry at ${registryUrl}. Is the server running?`))
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


