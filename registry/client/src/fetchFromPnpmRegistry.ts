import http from 'node:http'
import https from 'node:https'
import { URL } from 'node:url'

import type { LockfileObject } from '@pnpm/lockfile.types'
import type { PackageFilesIndex } from '@pnpm/store.cafs'
import { packForStorage, StoreIndex, storeIndexKey } from '@pnpm/store.index'
import { writeCafsFiles } from '@pnpm/worker'

import { decodeResponse, type ResponseMetadata } from './protocol.js'

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
}

/**
 * Fetch resolved dependencies from a pnpm registry server.
 *
 * 1. Read store integrities from the local store index
 * 2. Send POST /v1/install with dependencies + store integrities
 * 3. Parse the streaming response
 * 4. Write missing files to the local CAFS
 * 5. Write store index entries for new packages
 * 6. Return the lockfile for headless install
 */
export async function fetchFromPnpmRegistry (
  opts: FetchFromPnpmRegistryOptions
): Promise<FetchFromPnpmRegistryResult> {
  // 1. Read store integrities
  const storeIntegrities = readStoreIntegrities(opts.storeIndex)

  // 2. Build request body
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

  // 3. Send resolve request (returns JSON — lockfile + file indexes, no file content)
  const responseBuffer = await sendRequest(opts.registryUrl, '/v1/install', requestBody)
  const metadata: ResponseMetadata = JSON.parse(responseBuffer.toString('utf-8'))

  // 4. Fetch missing files in parallel batches via /v1/files
  if (metadata.missingDigests.length > 0) {
    await fetchFilesInParallel(opts.registryUrl, metadata, opts.storeDir)
  }

  // 5. Write store index entries for new packages
  writeStoreIndexEntries(metadata, opts.storeIndex)

  return {
    lockfile: metadata.lockfile,
    stats: metadata.stats,
  }
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

const BATCH_SIZE = 500 // files per worker batch
const PARALLEL_REQUESTS = 10 // concurrent HTTP requests to /v1/files
const FILES_PER_HTTP_REQUEST = 2000

async function fetchFilesInParallel (
  registryUrl: string,
  metadata: ResponseMetadata,
  storeDir: string
): Promise<void> {
  // Build digest info map from package files
  const digestInfo = new Map<string, { size: number, executable: boolean, mode: number }>()
  for (const pkgFiles of Object.values(metadata.packageFiles)) {
    for (const fileInfo of Object.values(pkgFiles.files)) {
      if (!digestInfo.has(fileInfo.digest)) {
        digestInfo.set(fileInfo.digest, {
          size: fileInfo.size,
          executable: (fileInfo.mode & 0o111) !== 0,
          mode: fileInfo.mode,
        })
      }
    }
  }

  // Split missing digests into HTTP request batches
  const httpBatches: Array<Array<{ digest: string, size: number, executable: boolean }>> = []
  let currentBatch: Array<{ digest: string, size: number, executable: boolean }> = []
  for (const digest of metadata.missingDigests) {
    const info = digestInfo.get(digest)
    if (!info) continue
    currentBatch.push({ digest, size: info.size, executable: info.executable })
    if (currentBatch.length >= FILES_PER_HTTP_REQUEST) {
      httpBatches.push(currentBatch)
      currentBatch = []
    }
  }
  if (currentBatch.length > 0) {
    httpBatches.push(currentBatch)
  }

  // Fetch HTTP batches with limited parallelism, dispatch to workers for CAFS writes
  let batchIdx = 0
  async function fetchNext (): Promise<void> {
    while (batchIdx < httpBatches.length) {
      const idx = batchIdx++
      const batch = httpBatches[idx]
      const reqBody = JSON.stringify({ digests: batch })
      const responseBuffer = await sendRequest(registryUrl, '/v1/files', reqBody) // eslint-disable-line no-await-in-loop
      const { files } = await decodeResponse(toAsyncIterable(responseBuffer)) // eslint-disable-line no-await-in-loop

      // Dispatch to worker threads for parallel CAFS writes
      const workerBatches: Array<Promise<number>> = []
      for (let i = 0; i < files.length; i += BATCH_SIZE) {
        const workerFiles = files.slice(i, i + BATCH_SIZE).map(f => ({
          buffer: f.content,
          digest: f.digest,
          mode: f.executable ? 0o755 : 0o644,
          size: f.size,
        }))
        workerBatches.push(writeCafsFiles({ storeDir, files: workerFiles }))
      }
      await Promise.all(workerBatches) // eslint-disable-line no-await-in-loop
    }
  }

  const fetchers = Array.from({ length: Math.min(PARALLEL_REQUESTS, httpBatches.length) }, () => fetchNext())
  await Promise.all(fetchers)
}

const REQUEST_TIMEOUT = 120_000 // 2 minutes

async function sendRequest (registryUrl: string, urlPath: string, body: string): Promise<Buffer> {
  const url = new URL(urlPath, registryUrl)
  const isHttps = url.protocol === 'https:'
  const requestFn = isHttps ? https.request : http.request

  return new Promise<Buffer>((resolve, reject) => {
    const req = requestFn(url, {
      method: 'POST',
      timeout: REQUEST_TIMEOUT,
      headers: {
        'Content-Type': 'application/json',
        'Content-Length': Buffer.byteLength(body),
        'Accept': 'application/x-pnpm-install',
      },
    }, (res) => {
      if (res.statusCode !== 200) {
        const chunks: Buffer[] = []
        res.on('data', (chunk: Buffer) => chunks.push(chunk))
        res.on('end', () => {
          const errBody = Buffer.concat(chunks).toString('utf-8')
          reject(new Error(`pnpm-registry responded with ${res.statusCode}: ${errBody}`))
        })
        return
      }

      const chunks: Buffer[] = []
      res.on('data', (chunk: Buffer) => chunks.push(chunk))
      res.on('end', () => resolve(Buffer.concat(chunks)))
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

function writeStoreIndexEntries (
  metadata: ResponseMetadata,
  storeIndex: StoreIndex
): void {
  const writes: Array<{ key: string, buffer: Uint8Array }> = []

  for (const [depPath, pkgFilesInfo] of Object.entries(metadata.packageFiles)) {
    // Use depPath as the pkgId — this matches what headlessInstall uses
    // to look up packages in the store via packageIdFromSnapshot().
    const key = storeIndexKey(pkgFilesInfo.integrity, depPath)

    // Check if already in index
    if (storeIndex.has(key)) continue

    // Build PackageFilesIndex
    const files = new Map<string, { checkedAt: number, digest: string, mode: number, size: number }>()
    for (const [relativePath, fileInfo] of Object.entries(pkgFilesInfo.files)) {
      files.set(relativePath, {
        checkedAt: Date.now(),
        digest: fileInfo.digest,
        mode: fileInfo.mode,
        size: fileInfo.size,
      })
    }

    const packageFilesIndex: PackageFilesIndex = {
      algo: pkgFilesInfo.algo,
      files,
    }

    writes.push({
      key,
      buffer: packForStorage(packageFilesIndex) as Uint8Array,
    })
  }

  if (writes.length > 0) {
    storeIndex.setRawMany(writes)
  }
}


async function * toAsyncIterable (buffer: Buffer): AsyncIterable<Buffer> {
  yield buffer
}
