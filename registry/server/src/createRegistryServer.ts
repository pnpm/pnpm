import { promises as fs } from 'node:fs'
import http from 'node:http'
import os from 'node:os'
import path from 'node:path'

import { createClient } from '@pnpm/installing.client'
import type { InstallOptions } from '@pnpm/installing.deps-installer'
import { install } from '@pnpm/installing.deps-installer'
import { readWantedLockfile, writeWantedLockfile } from '@pnpm/lockfile.fs'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { getFilePathByModeInCafs, type PackageFilesIndex } from '@pnpm/store.cafs'
import { createPackageStore, type StoreController } from '@pnpm/store.controller'
import { StoreIndex } from '@pnpm/store.index'
import type { Registries } from '@pnpm/types'

import { buildIntegrityIndex, computeDiff } from './diff.js'
import { encodeResponse, type MissingFile } from './protocol.js'

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
  integrityIndex: Map<string, PackageFilesIndex>
}

export async function createRegistryServer (opts: RegistryServerOptions): Promise<http.Server> {
  const storeDir = opts.storeDir
  const cacheDir = opts.cacheDir
  const registries: Registries = opts.registries ?? { default: 'https://registry.npmjs.org/' }

  await fs.mkdir(storeDir, { recursive: true })
  await fs.mkdir(cacheDir, { recursive: true })

  const storeIndex = new StoreIndex(storeDir)

  const { resolve, fetchers, clearResolutionCache } = createClient({
    cacheDir,
    storeDir,
    storeIndex,
    registries,
    configByUri: {},
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

  const ctx: ServerContext = {
    storeController,
    storeIndex,
    storeDir,
    cacheDir,
    registries,
    integrityIndex,
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
  const body = await readBody(req)
  const request: InstallRequest = JSON.parse(body)

  const tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-registry-'))

  try {
    // Write package.json for the temp project
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

    // Write existing lockfile if provided (for incremental resolution)
    if (request.lockfile) {
      await writeWantedLockfile(tmpDir, request.lockfile)
    }

    // Phase 1: Resolve only (no fetching, no linking — fast).
    // lockfileIncludeTarballUrl gives us download URLs for phase 2.
    await install(manifest, {
      dir: tmpDir,
      lockfileDir: tmpDir,
      storeController: ctx.storeController,
      storeDir: ctx.storeDir,
      cacheDir: ctx.cacheDir,
      registries: ctx.registries,
      ignoreScripts: true,
      lockfileOnly: true,
      lockfileIncludeTarballUrl: true,
      saveLockfile: true,
      preferFrozenLockfile: false,
    } as InstallOptions)

    // Read the resolved lockfile
    const resolvedLockfile = await readWantedLockfile(tmpDir, {
      ignoreIncompatible: false,
    })

    if (!resolvedLockfile) {
      res.writeHead(500, { 'Content-Type': 'application/json' })
      res.end(JSON.stringify({ error: 'Resolution produced no lockfile' }))
      return
    }

    // Remap importer IDs: the server resolved in a temp dir so importers
    // are keyed by the temp path. Remap them to "." so the client's
    // headless install can find the project.
    const importerEntries = Object.entries(resolvedLockfile.importers)
    if (importerEntries.length === 1) {
      const [, snapshot] = importerEntries[0]
      resolvedLockfile.importers = { '.': snapshot } as typeof resolvedLockfile.importers
    }

    // Phase 2: Fetch tarballs into the server's store for packages we
    // don't have yet. On first request this downloads everything; on
    // subsequent requests the store is hot and this is a no-op.
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

    // Compute the file diff using only what the server already has
    const { metadata } = computeDiff(
      resolvedLockfile,
      request.storeIntegrities ?? [],
      integrityIndex,
      ctx.storeDir
    )

    // Return JSON only — file contents are fetched via parallel /v1/files requests
    res.writeHead(200, { 'Content-Type': 'application/json' })
    res.end(JSON.stringify(metadata))
  } finally {
    await fs.rm(tmpDir, { recursive: true, force: true })
  }
}

/**
 * Handle POST /v1/files — serve a batch of files by digest.
 * The client makes multiple parallel requests to this endpoint.
 */
async function handleFiles (
  req: http.IncomingMessage,
  res: http.ServerResponse,
  ctx: ServerContext
): Promise<void> {
  const body = await readBody(req)
  const { digests } = JSON.parse(body) as { digests: Array<{ digest: string, size: number, executable: boolean }> }

  const missingFiles: MissingFile[] = digests.map((d) => ({
    digest: d.digest,
    size: d.size,
    executable: d.executable,
    cafsPath: getFilePathByModeInCafs(ctx.storeDir, d.digest, d.executable ? 0o755 : 0o644),
  }))

  await encodeResponse(res, null, missingFiles)
}

function readBody (req: http.IncomingMessage): Promise<string> {
  return new Promise<string>((resolve, reject) => {
    const chunks: Buffer[] = []
    req.on('data', (chunk: Buffer) => chunks.push(chunk))
    req.on('end', () => resolve(Buffer.concat(chunks).toString('utf-8')))
    req.on('error', reject)
  })
}

