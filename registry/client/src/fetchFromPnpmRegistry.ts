import { promises as fs } from 'node:fs'
import http from 'node:http'
import https from 'node:https'
import path from 'node:path'
import { URL } from 'node:url'

import type { LockfileObject } from '@pnpm/lockfile.types'
import { getFilePathByModeInCafs, type PackageFilesIndex } from '@pnpm/store.cafs'
import { packForStorage, StoreIndex, storeIndexKey } from '@pnpm/store.index'

import { type DecodedFile, decodeResponse, type ResponseMetadata } from './protocol.js'

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

  // 3. Send request and get response stream
  const responseBuffer = await sendRequest(opts.registryUrl, requestBody)

  // 4. Decode the response
  const { metadata, files } = await decodeResponse(toAsyncIterable(responseBuffer))

  // 5. Write missing files to CAFS
  await writeFilesToCafs(files, opts.storeDir)

  // 6. Write store index entries for new packages
  writeStoreIndexEntries(metadata, opts.storeIndex)

  return {
    lockfile: metadata.lockfile,
    stats: metadata.stats,
  }
}

function readStoreIntegrities (storeIndex: StoreIndex): string[] {
  const integrities: string[] = []
  for (const [key] of storeIndex.entries()) {
    const tabIdx = key.indexOf('\t')
    if (tabIdx === -1) continue
    const integrity = key.slice(0, tabIdx)
    if (!integrities.includes(integrity)) {
      integrities.push(integrity)
    }
  }
  return integrities
}

const REQUEST_TIMEOUT = 120_000 // 2 minutes

async function sendRequest (registryUrl: string, body: string): Promise<Buffer> {
  const url = new URL('/v1/install', registryUrl)
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

async function writeFilesToCafs (files: DecodedFile[], storeDir: string): Promise<void> {
  await Promise.all(files.map((file) => writeFileToCafs(file, storeDir)))
}

async function writeFileToCafs (file: DecodedFile, storeDir: string): Promise<void> {
  const mode = file.executable ? 0o755 : 0o644
  const cafsPath = getFilePathByModeInCafs(storeDir, file.digest, mode)

  // Ensure directory exists
  const dir = path.dirname(cafsPath)
  await fs.mkdir(dir, { recursive: true })

  // Write atomically: temp file + rename
  const tmpPath = `${cafsPath}.${process.pid}`
  try {
    await fs.writeFile(tmpPath, file.content, { mode })
    await fs.rename(tmpPath, cafsPath)
  } catch (err: unknown) {
    // If file already exists (race condition), that's fine
    if (isErrnoException(err) && err.code === 'EEXIST') return
    // If rename failed because target already exists, that's also fine
    try {
      await fs.unlink(tmpPath)
    } catch {}
    // Check if the target already exists
    try {
      await fs.stat(cafsPath)
      return // file exists, skip
    } catch {}
    throw err
  }
}

function writeStoreIndexEntries (
  metadata: ResponseMetadata,
  storeIndex: StoreIndex
): void {
  const writes: Array<{ key: string, buffer: Uint8Array }> = []

  for (const [depPath, pkgFilesInfo] of Object.entries(metadata.packageFiles)) {
    const { name, version } = parseDepPath(depPath)
    const pkgId = `registry.npmjs.org/${name}@${version}`
    const key = storeIndexKey(pkgFilesInfo.integrity, pkgId)

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

function parseDepPath (depPath: string): { name: string, version: string } {
  // depPath format: "/name/version" or "/@scope/name/version"
  const parts = depPath.slice(1).split('/')
  if (parts[0].startsWith('@')) {
    return {
      name: `${parts[0]}/${parts[1]}`,
      version: parts[2],
    }
  }
  return {
    name: parts[0],
    version: parts[1],
  }
}

function isErrnoException (err: unknown): err is NodeJS.ErrnoException {
  return err instanceof Error && 'code' in err
}

async function * toAsyncIterable (buffer: Buffer): AsyncIterable<Buffer> {
  yield buffer
}
