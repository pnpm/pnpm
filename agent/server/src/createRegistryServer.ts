import { promises as fs, readFileSync } from 'node:fs'
import http from 'node:http'
import os from 'node:os'
import path from 'node:path'
import { createGzip } from 'node:zlib'

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

  // Close SQLite databases when the server shuts down.
  // On Windows, files can't be deleted while SQLite has them open.
  server.on('close', () => {
    fileStore.close()
    metadataStore.close()
    storeIndex.close()
  })

  return server
}

interface InstallRequestProject {
  dir: string
  dependencies?: Record<string, string>
  devDependencies?: Record<string, string>
}

interface InstallRequest {
  /** Single project (legacy) */
  dependencies?: Record<string, string>
  devDependencies?: Record<string, string>
  /** Multiple projects (workspace) */
  projects?: InstallRequestProject[]
  overrides?: Record<string, string>
  peerDependencyRules?: Record<string, unknown>
  nodeVersion?: string
  os?: string
  arch?: string
  minimumReleaseAge?: number
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

  res.writeHead(200, {
    'Content-Type': 'application/x-ndjson',
    'Transfer-Encoding': 'chunked',
  })

  // Wrap the store controller to intercept package resolutions.
  // As each package resolves, stream file digest lines immediately.
  // Only D lines here — I lines come from the diff computation which
  // uses depPath-based keys matching what headless install expects.
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

  const tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-agent-'))

  try {
    // Build project list — support both single-project and workspace
    const projects = request.projects ?? [{
      dir: '.',
      dependencies: request.dependencies,
      devDependencies: request.devDependencies,
    }]

    // Create package.json for each project
    await Promise.all(projects.map(async (project) => {
      const projectDir = path.join(tmpDir, project.dir)
      await fs.mkdir(projectDir, { recursive: true })
      await fs.writeFile(
        path.join(projectDir, 'package.json'),
        JSON.stringify({
          name: `pnpm-agent-resolve-${project.dir.replace(/[/\\]/g, '-')}`,
          version: '0.0.0',
          dependencies: project.dependencies ?? {},
          devDependencies: project.devDependencies ?? {},
        }, null, 2)
      )
    }))

    if (request.lockfile) {
      await writeWantedLockfile(tmpDir, request.lockfile)
    }

    // Resolution — the wrapped store controller streams digest info
    // to the client as each package resolves.
    if (projects.length === 1) {
      const manifest = {
        name: 'pnpm-agent-resolve',
        version: '0.0.0',
        dependencies: projects[0].dependencies ?? {},
        devDependencies: projects[0].devDependencies ?? {},
      }
      await install(manifest, {
        dir: path.join(tmpDir, projects[0].dir),
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
        minimumReleaseAge: request.minimumReleaseAge,
      } as InstallOptions)
    } else {
      // Workspace: create pnpm-workspace.yaml and resolve all projects
      const packagesYaml = `packages:\n${projects.map(p => `  - '${p.dir}'`).join('\n')}\n`
      await fs.writeFile(path.join(tmpDir, 'pnpm-workspace.yaml'), packagesYaml)
      const { mutateModules } = await import('@pnpm/installing.deps-installer')
      await mutateModules(
        projects.map(p => ({
          mutation: 'install' as const,
          rootDir: path.join(tmpDir, p.dir) as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        })),
        {
          allProjects: projects.map((p, i) => ({
            buildIndex: i,
            manifest: {
              name: `pnpm-agent-resolve-${p.dir.replace(/[/\\]/g, '-')}`,
              version: '0.0.0',
              dependencies: p.dependencies ?? {},
              devDependencies: p.devDependencies ?? {},
            },
            rootDir: path.join(tmpDir, p.dir) as any, // eslint-disable-line @typescript-eslint/no-explicit-any
          })),
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
          minimumReleaseAge: request.minimumReleaseAge,
        } as any // eslint-disable-line @typescript-eslint/no-explicit-any
      )
    }

    const resolvedLockfile = await readWantedLockfile(tmpDir, {
      ignoreIncompatible: false,
    })

    if (!resolvedLockfile) {
      res.end('E\t{"error":"Resolution produced no lockfile"}\n')
      return
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

    // Emit remaining digests + index entries for packages not caught
    // by the wrapper (e.g. resolved from a cached lockfile path).
    const { missingFiles, packageIndexBuffers } = computeDiff(
      resolvedLockfile,
      request.storeIntegrities ?? [],
      integrityIndex,
      ctx.storeDir
    )

    for (const f of missingFiles) {
      const dedupeKey = `${f.digest}:${f.executable ? 'x' : ''}`
      if (!emittedDigests.has(dedupeKey)) {
        emittedDigests.add(dedupeKey)
        res.write(`D\t${f.digest}\t${f.size}\t${f.executable ? 1 : 0}\n`)
      }
    }

    for (const [depPath, { integrity, rawBuffer }] of packageIndexBuffers) {
      const pkgId = depPath.includes('(') ? depPath.substring(0, depPath.indexOf('(')) : depPath
      const key = `${integrity}\t${pkgId}`
      res.write(`I\t${key}\t${Buffer.from(rawBuffer).toString('base64')}\n`)
    }

    // Send lockfile AFTER all I lines so the client has all index
    // entries before it resolves and starts headless install.
    const stats = {
      totalPackages: Object.keys(resolvedLockfile.packages ?? {}).length,
      alreadyInStore: 0,
      packagesToFetch: 0,
      filesInNewPackages: 0,
      filesAlreadyInCafs: 0,
      filesToDownload: emittedDigests.size,
      downloadBytes: 0,
    }
    res.write(`L\t${JSON.stringify({ lockfile: resolvedLockfile, stats })}\n`)

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

  // Stream file entries through gzip — no buffering. The server reads
  // one file at a time and pipes it through gzip to the response.
  // The worker starts writing to CAFS as soon as the first bytes arrive.
  res.writeHead(200, {
    'Content-Type': 'application/x-pnpm-install',
    'Content-Encoding': 'gzip',
    'Transfer-Encoding': 'chunked',
  })

  const gzip = createGzip({ level: 1 })
  gzip.pipe(res)

  // JSON header
  const jsonBuffer = Buffer.from('{}', 'utf-8')
  const lengthBuf = Buffer.alloc(4)
  lengthBuf.writeUInt32BE(jsonBuffer.length, 0)
  gzip.write(lengthBuf)
  gzip.write(jsonBuffer)

  // File entries — streamed through gzip
  for (const d of digests) {
    const cached = ctx.fileStore.get(d.digest)
    const content = cached
      ? cached.content
      : readFileSync(getFilePathByModeInCafs(ctx.storeDir, d.digest, d.executable ? 0o755 : 0o644))

    const header = Buffer.alloc(69)
    Buffer.from(d.digest, 'hex').copy(header, 0)
    header.writeUInt32BE(content.length, 64)
    header[68] = d.executable ? 0x01 : 0x00

    gzip.write(header)
    gzip.write(content)
  }

  // End marker + close gzip stream (which ends the response)
  gzip.end(Buffer.alloc(64, 0))
}

function readBody (req: http.IncomingMessage): Promise<string> {
  return new Promise<string>((resolve, reject) => {
    const chunks: Buffer[] = []
    req.on('data', (chunk: Buffer) => chunks.push(chunk))
    req.on('end', () => resolve(Buffer.concat(chunks).toString('utf-8')))
    req.on('error', reject)
  })
}

