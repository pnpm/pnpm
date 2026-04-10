import { promises as fs, readFileSync } from 'node:fs'
import http from 'node:http'
import os from 'node:os'
import path from 'node:path'

import { createClient } from '@pnpm/installing.client'
import type { InstallOptions } from '@pnpm/installing.deps-installer'
import { install } from '@pnpm/installing.deps-installer'
import { readWantedLockfile, writeWantedLockfile } from '@pnpm/lockfile.fs'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { getFilePathByModeInCafs } from '@pnpm/store.cafs'
import { createPackageStore, type StoreController } from '@pnpm/store.controller'
import { StoreIndex } from '@pnpm/store.index'
import type { Registries } from '@pnpm/types'

import { buildIntegrityIndex, computeDiff, getFilesEntries, type IntegrityEntry } from './diff.js'
import { FileStore } from './fileStore.js'
import { MetadataStore } from './metadataStore.js'

export interface RegistryServerOptions {
  /** Directory for the server's content-addressable store */
  storeDir: string
  /** Directory for metadata cache */
  cacheDir: string
  /** Upstream registries to resolve from */
  registries?: Registries
  /** Port to listen on */
  port?: number
}

interface ServerContext {
  storeController: StoreController
  storeIndex: StoreIndex
  storeDir: string
  cacheDir: string
  registries: Registries
  integrityIndex: Map<string, IntegrityEntry>
  fileStore: FileStore
}

export async function createRegistryServer (opts: RegistryServerOptions): Promise<http.Server> {
  const storeDir = opts.storeDir
  const cacheDir = opts.cacheDir
  const registries: Registries = opts.registries ?? { default: 'https://registry.npmjs.org/' }

  await fs.mkdir(storeDir, { recursive: true })
  await fs.mkdir(cacheDir, { recursive: true })

  const storeIndex = new StoreIndex(storeDir)

  // Pre-populate metadata cache from .jsonl files into SQLite.
  // This makes resolution fast — indexed DB lookup instead of
  // reading/parsing hundreds of multi-MB JSON files from disk.
  const metadataStore = new MetadataStore(path.join(storeDir, 'metadata.db'))
  const metaImported = metadataStore.importFromCacheDir(cacheDir)
  if (metaImported > 0) {
    console.log(`  imported ${metaImported} metadata entries to SQLite`)
  }

  const { resolve, fetchers, clearResolutionCache } = createClient({
    cacheDir,
    storeDir,
    storeIndex,
    registries,
    configByUri: {},
    metaCache: metadataStore as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    retry: {
      retries: 3,
      factor: 10,
      maxTimeout: 60_000,
      minTimeout: 10_000,
    },
    strictSsl: true,
  })

  const storeController = createPackageStore(resolve, fetchers, {
    cacheDir,
    storeDir,
    storeIndex,
    verifyStoreIntegrity: true,
    virtualStoreDirMaxLength: 120,
    clearResolutionCache,
  })

  const integrityIndex = buildIntegrityIndex(storeIndex)
  const fileStore = new FileStore(path.join(storeDir, 'files.db'))

  const ctx: ServerContext = {
    storeController,
    storeIndex,
    storeDir,
    cacheDir,
    registries,
    integrityIndex,
    fileStore,
  }

  const server = http.createServer(async (req, res) => {
    try {
      if (req.method === 'POST' && req.url === '/v1/install') {
        await handleInstall(req, res, ctx)
      } else if (req.method === 'POST' && req.url === '/v1/files') {
        await handleFiles(req, res, ctx)
      } else {
        res.writeHead(404, { 'Content-Type': 'application/json' })
        res.end(JSON.stringify({ error: 'Not found' }))
      }
    } catch (err: unknown) {
      if (!res.headersSent) {
        res.writeHead(500, { 'Content-Type': 'application/json' })
      }
      const message = err instanceof Error ? err.message : 'Internal server error'
      res.end(JSON.stringify({ error: message }))
    }
  })

  return server
}

interface InstallRequest {
  dependencies?: Record<string, string>
  devDependencies?: Record<string, string>
  overrides?: Record<string, string>
  peerDependencyRules?: Record<string, unknown>
  nodeVersion?: string
  os?: string
  arch?: string
  lockfile?: LockfileObject
  storeIntegrities?: string[]
}

async function handleInstall (
  req: http.IncomingMessage,
  res: http.ServerResponse,
  ctx: ServerContext
): Promise<void> {
  const t0 = performance.now()
  const body = await readBody(req)
  const request: InstallRequest = JSON.parse(body)

  // Build the set of integrities the client already has, so we can
  // compute per-package diffs as packages resolve.
  const clientIntegrities = new Set(request.storeIntegrities ?? [])

  const emittedDigests = new Set<string>()

  // Start streaming NDJSON — digests stream as packages resolve,
  // lockfile + index entries come at the end.
  res.writeHead(200, {
    'Content-Type': 'application/x-ndjson',
    'Transfer-Encoding': 'chunked',
  })

  // Wrap the store controller to intercept package resolutions.
  // As each package resolves, stream digest info directly to the
  // response stream. Node.js handles internal buffering at the socket.
  const wrappedStoreController: StoreController = {
    ...ctx.storeController,
    requestPackage: async (...args: Parameters<StoreController['requestPackage']>) => {
      const result = await ctx.storeController.requestPackage(...args)
      const integrity = (result.body as any)?.resolution?.integrity as string | undefined // eslint-disable-line @typescript-eslint/no-explicit-any
      if (integrity && !clientIntegrities.has(integrity)) {
        const entry = ctx.integrityIndex.get(integrity)
        if (entry) {
          for (const [, fileInfo] of getFilesEntries(entry.decoded)) {
            const executable = (fileInfo.mode & 0o111) !== 0
            const dedupeKey = `${fileInfo.digest}:${executable ? 'x' : ''}`
            if (!emittedDigests.has(dedupeKey)) {
              emittedDigests.add(dedupeKey)
              res.write(`D\t${fileInfo.digest}\t${fileInfo.size}\t${executable ? 1 : 0}\n`)
            }
          }
        }
      }
      return result
    },
  }

  const tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-registry-'))

  try {
    const manifest = {
      name: 'pnpm-registry-resolve',
      version: '0.0.0',
      dependencies: request.dependencies ?? {},
      devDependencies: request.devDependencies ?? {},
    }
    await fs.writeFile(
      path.join(tmpDir, 'package.json'),
      JSON.stringify(manifest, null, 2)
    )

    if (request.lockfile) {
      await writeWantedLockfile(tmpDir, request.lockfile)
    }

    // Resolution — the wrapped store controller streams digest info
    // to the client as each package resolves.
    await install(manifest, {
      dir: tmpDir,
      lockfileDir: tmpDir,
      storeController: wrappedStoreController,
      storeDir: ctx.storeDir,
      cacheDir: ctx.cacheDir,
      registries: ctx.registries,
      ignoreScripts: true,
      lockfileOnly: true,
      lockfileIncludeTarballUrl: true,
      saveLockfile: true,
      preferFrozenLockfile: false,
    } as InstallOptions)

    const resolvedLockfile = await readWantedLockfile(tmpDir, {
      ignoreIncompatible: false,
    })

    if (!resolvedLockfile) {
      res.end('E\t{"error":"Resolution produced no lockfile"}\n')
      return
    }

    const importerEntries = Object.entries(resolvedLockfile.importers)
    if (importerEntries.length === 1) {
      const [, snapshot] = importerEntries[0]
      resolvedLockfile.importers = { '.': snapshot } as typeof resolvedLockfile.importers
    }

    // Fetch tarballs for new packages (hot server = no-op)
    const integrityIndexBefore = buildIntegrityIndex(ctx.storeIndex)
    const fetchPromises: Array<Promise<void>> = []
    for (const [, pkgSnapshot] of Object.entries(resolvedLockfile.packages ?? {})) {
      const resolution = pkgSnapshot.resolution as { integrity?: string, tarball?: string } | undefined
      if (!resolution?.integrity || !resolution?.tarball) continue
      if (integrityIndexBefore.has(resolution.integrity)) continue

      fetchPromises.push((async () => {
        const result = await ctx.storeController.fetchPackage({
          force: false,
          lockfileDir: tmpDir,
          pkg: {
            id: resolution.tarball as any, // eslint-disable-line @typescript-eslint/no-explicit-any
            name: '',
            version: '',
            resolution: resolution as any, // eslint-disable-line @typescript-eslint/no-explicit-any
          },
        })
        if (result.fetching) {
          await result.fetching()
        }
      })())
    }
    if (fetchPromises.length > 0) {
      await Promise.all(fetchPromises)
    }

    const integrityIndex = buildIntegrityIndex(ctx.storeIndex)
    ctx.integrityIndex = integrityIndex

    // Compute remaining diff for any packages not caught by the wrapper
    // (e.g. packages resolved from a cached lockfile path)
    const { metadata, packageIndexBuffers, missingFiles } = computeDiff(
      resolvedLockfile,
      request.storeIntegrities ?? [],
      integrityIndex,
      ctx.storeDir
    )

    // Emit any digests not yet streamed
    for (const f of metadata.missingFiles) {
      const dedupeKey = `${f.digest}:${f.executable ? 'x' : ''}`
      if (!emittedDigests.has(dedupeKey)) {
        emittedDigests.add(dedupeKey)
        res.write(`D\t${f.digest}\t${f.size}\t${f.executable ? 1 : 0}\n`)
      }
    }

    // Sync to SQLite in background
    if (missingFiles.length > 0) {
      setImmediate(() => {
        ctx.fileStore.importManyFromCafs(missingFiles.map(f => ({
          digest: f.digest,
          cafsPath: f.cafsPath,
          executable: f.executable,
        })))
      })
    }

    // Send lockfile + stats
    const lockfilePayload = JSON.stringify({
      lockfile: metadata.lockfile,
      stats: metadata.stats,
    })
    res.write(`L\t${lockfilePayload}\n`)

    // Send pre-packed index entries
    for (const [depPath, { integrity, rawBuffer }] of packageIndexBuffers) {
      const pkgId = depPath.includes('(') ? depPath.substring(0, depPath.indexOf('(')) : depPath
      const key = `${integrity}\t${pkgId}`
      // Base64-encode the msgpack buffer for NDJSON transport
      res.write(`I\t${key}\t${Buffer.from(rawBuffer).toString('base64')}\n`)
    }

    res.end()
    console.log(`[SERVER /v1/install] ${(performance.now() - t0).toFixed(0)}ms digests=${emittedDigests.size}`)
  } finally {
    await fs.rm(tmpDir, { recursive: true, force: true })
  }
}

/**
 * Handle POST /v1/files — serve a batch of files by digest.
 * Reads from SQLite file store for fast batch access (one DB
 * vs 33K individual readFileSync calls).
 */
async function handleFiles (
  req: http.IncomingMessage,
  res: http.ServerResponse,
  ctx: ServerContext
): Promise<void> {
  const body = await readBody(req)
  const { digests } = JSON.parse(body) as { digests: Array<{ digest: string, size: number, executable: boolean }> }

  // Build binary response — same format as encodeResponse but reads from SQLite
  const parts: Buffer[] = []

  // JSON metadata (empty for file-only response)
  const jsonBuffer = Buffer.from('{}', 'utf-8')
  const lengthBuf = Buffer.alloc(4)
  lengthBuf.writeUInt32BE(jsonBuffer.length, 0)
  parts.push(lengthBuf)
  parts.push(jsonBuffer)

  for (const d of digests) {
    const file = ctx.fileStore.get(d.digest)
    let content: Buffer
    if (file) {
      content = file.content
    } else {
      const cafsPath = getFilePathByModeInCafs(ctx.storeDir, d.digest, d.executable ? 0o755 : 0o644)
      content = readFileSync(cafsPath)
      // Cache in SQLite for next time
      ctx.fileStore.importFromCafs(d.digest, cafsPath, d.executable)
    }

    const digestBuf = Buffer.from(d.digest, 'hex')
    const sizeBuf = Buffer.alloc(4)
    sizeBuf.writeUInt32BE(content.length, 0)
    const modeBuf = Buffer.alloc(1)
    modeBuf[0] = d.executable ? 0x01 : 0x00

    parts.push(digestBuf)
    parts.push(sizeBuf)
    parts.push(modeBuf)
    parts.push(content)
  }

  parts.push(Buffer.alloc(64, 0)) // end marker

  const payload = Buffer.concat(parts)
  res.writeHead(200, {
    'Content-Type': 'application/x-pnpm-install',
    'Content-Length': payload.length,
  })
  res.end(payload)
}

function readBody (req: http.IncomingMessage): Promise<string> {
  return new Promise<string>((resolve, reject) => {
    const chunks: Buffer[] = []
    req.on('data', (chunk: Buffer) => chunks.push(chunk))
    req.on('end', () => resolve(Buffer.concat(chunks).toString('utf-8')))
    req.on('error', reject)
  })
}

