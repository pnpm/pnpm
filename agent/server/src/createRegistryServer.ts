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
  /**
   * Skip importing .jsonl metadata files into SQLite on startup. Set this
   * to true in forked cluster workers when the primary has already run
   * {@link warmupMetadataCache} so workers don't re-scan the cache dir.
   */
  skipCacheImport?: boolean
}

/**
 * Import .jsonl metadata files from the cache directory into SQLite.
 *
 * This is idempotent and safe to run multiple times, but it's expensive on
 * large stores. When running with `cluster`, invoke this once in the primary
 * before forking workers so every worker doesn't independently re-scan.
 */
export async function warmupMetadataCache (opts: {
  storeDir: string
  cacheDir: string
}): Promise<number> {
  await fs.mkdir(opts.storeDir, { recursive: true })
  await fs.mkdir(opts.cacheDir, { recursive: true })
  const metadataStore = new MetadataStore(path.join(opts.storeDir, 'metadata.db'))
  try {
    return metadataStore.importFromCacheDir(opts.cacheDir)
  } finally {
    metadataStore.close()
  }
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
  // Skipped when `skipCacheImport` is set (e.g. cluster workers where the
  // primary already ran this step).
  const metadataStore = new MetadataStore(path.join(storeDir, 'metadata.db'))
  if (!opts.skipCacheImport) {
    const metaImported = metadataStore.importFromCacheDir(cacheDir)
    if (metaImported > 0) {
      console.log(`  imported ${metaImported} metadata entries to SQLite`)
    }
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
      const message = err instanceof Error ? err.message : 'Internal server error'
      const statusCode = typeof (err as { statusCode?: unknown })?.statusCode === 'number'
        ? (err as { statusCode: number }).statusCode
        : 500
      if (!res.headersSent) {
        res.writeHead(statusCode, { 'Content-Type': 'application/json' })
        res.end(JSON.stringify({ error: message }))
        return
      }
      // Headers already sent — emit an NDJSON error line for /v1/install so the
      // client can reject with a structured error. For the binary /v1/files
      // stream there's no recoverable framing, so just destroy the response.
      const contentType = res.getHeader('Content-Type')
      if (typeof contentType === 'string' && contentType.includes('ndjson')) {
        res.end(`E\t${JSON.stringify({ error: message })}\n`)
      } else {
        res.destroy()
      }
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
  const body = await readBody(req, INSTALL_BODY_MAX_BYTES)
  let request: InstallRequest
  try {
    request = JSON.parse(body)
  } catch (err: unknown) {
    res.writeHead(400, { 'Content-Type': 'application/json' })
    const message = err instanceof Error ? err.message : 'Invalid JSON body'
    res.end(JSON.stringify({ error: message }))
    return
  }

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

    // Validate every `dir` is a safe relative path. A malicious client
    // could pass "../../etc" or an absolute path, which would escape
    // tmpDir and let the server write files outside its workspace.
    for (const project of projects) {
      if (!isSafeRelativeDir(project.dir)) {
        res.end(`E\t${JSON.stringify({ error: `Invalid project dir: ${project.dir}` })}\n`)
        return
      }
    }

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
    const diff = computeDiff(
      resolvedLockfile,
      request.storeIntegrities ?? [],
      integrityIndex,
      ctx.storeDir
    )

    for (const f of diff.missingFiles) {
      const dedupeKey = `${f.digest}:${f.executable ? 'x' : ''}`
      if (!emittedDigests.has(dedupeKey)) {
        emittedDigests.add(dedupeKey)
        res.write(`D\t${f.digest}\t${f.size}\t${f.executable ? 1 : 0}\n`)
      }
    }

    for (const [depPath, { integrity, rawBuffer }] of diff.packageIndexBuffers) {
      const pkgId = depPath.includes('(') ? depPath.substring(0, depPath.indexOf('(')) : depPath
      const key = `${integrity}\t${pkgId}`
      res.write(`I\t${key}\t${Buffer.from(rawBuffer).toString('base64')}\n`)
    }

    // Send lockfile AFTER all I lines so the client has all index
    // entries before it resolves and starts headless install.
    res.write(`L\t${JSON.stringify({ lockfile: resolvedLockfile, stats: diff.metadata.stats })}\n`)

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
  const body = await readBody(req, FILES_BODY_MAX_BYTES)
  let parsed: { digests?: unknown }
  try {
    parsed = JSON.parse(body) as { digests?: unknown }
  } catch (err: unknown) {
    res.writeHead(400, { 'Content-Type': 'application/json' })
    res.end(JSON.stringify({ error: err instanceof Error ? err.message : 'Invalid JSON body' }))
    return
  }

  if (!Array.isArray(parsed.digests)) {
    res.writeHead(400, { 'Content-Type': 'application/json' })
    res.end(JSON.stringify({ error: '`digests` must be an array' }))
    return
  }

  // Validate each entry's shape up front. Reading `d.digest` on a `null` or
  // `{ digest: 123 }` entry would otherwise throw and surface as a 500 after
  // headers may have been written. A valid sha512 digest is 128 lowercase hex
  // characters (64 bytes); also reject the all-zero digest since it collides
  // with the 64-byte end-of-stream marker.
  const digests: Array<{ digest: string, executable: boolean }> = []
  for (let i = 0; i < parsed.digests.length; i++) {
    const entry = parsed.digests[i]
    if (entry === null || typeof entry !== 'object') {
      res.writeHead(400, { 'Content-Type': 'application/json' })
      res.end(JSON.stringify({ error: `digests[${i}] must be an object` }))
      return
    }
    const { digest, executable } = entry as { digest?: unknown, executable?: unknown }
    if (typeof digest !== 'string' || !isValidSha512Hex(digest)) {
      res.writeHead(400, { 'Content-Type': 'application/json' })
      res.end(JSON.stringify({ error: `digests[${i}].digest must be a valid sha512 hex string` }))
      return
    }
    if (typeof executable !== 'boolean') {
      res.writeHead(400, { 'Content-Type': 'application/json' })
      res.end(JSON.stringify({ error: `digests[${i}].executable must be a boolean` }))
      return
    }
    digests.push({ digest, executable })
  }

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

  // File entries — streamed through gzip. On a SQLite cache miss we read
  // from CAFS and also insert the already-read buffer into the file store
  // so subsequent batches can serve the same digest from SQLite (one
  // `.get()` vs `readFileSync`) without re-reading from disk.
  const missing: Array<{ digest: string, content: Buffer, executable: boolean }> = []
  for (const d of digests) {
    const cached = ctx.fileStore.get(d.digest)
    let content: Buffer
    if (cached) {
      content = cached.content
    } else {
      content = readFileSync(getFilePathByModeInCafs(ctx.storeDir, d.digest, d.executable ? 0o755 : 0o644))
      missing.push({ digest: d.digest, content, executable: d.executable })
    }

    const header = Buffer.alloc(69)
    Buffer.from(d.digest, 'hex').copy(header, 0)
    header.writeUInt32BE(content.length, 64)
    header[68] = d.executable ? 0x01 : 0x00

    gzip.write(header)
    gzip.write(content)
  }
  if (missing.length > 0) {
    ctx.fileStore.importMany(missing)
  }

  // End marker + close gzip stream (which ends the response)
  gzip.end(Buffer.alloc(64, 0))
}

const SHA512_HEX_RE = /^[0-9a-f]{128}$/
// The wire protocol uses a 64-byte all-zero buffer as the end-of-stream marker.
// A sha512 whose hex representation is all zeros would serialize to the same
// 64 bytes and collide with the framing marker, so treat it as invalid input.
const ALL_ZERO_SHA512_HEX = '0'.repeat(128)

function isValidSha512Hex (digest: string): boolean {
  return typeof digest === 'string' && SHA512_HEX_RE.test(digest) && digest !== ALL_ZERO_SHA512_HEX
}

// Characters that would let a crafted `dir` break out of the YAML single-quoted
// scalar we emit into `pnpm-workspace.yaml`, or inject shell metacharacters.
// A legitimate project directory never contains any of these.
const UNSAFE_DIR_CHAR_RE = /[\x00-\x1f'"`\\]/ // eslint-disable-line no-control-regex

function isSafeRelativeDir (dir: string): boolean {
  if (typeof dir !== 'string' || dir.length === 0) return false
  if (UNSAFE_DIR_CHAR_RE.test(dir)) return false
  if (dir === '.') return true
  if (path.isAbsolute(dir)) return false
  // Reject Windows drive letters (e.g. "C:foo", "C:\\foo") even on POSIX.
  if (/^[a-z]:/i.test(dir)) return false
  const normalized = path.posix.normalize(dir.replace(/\\/g, '/'))
  if (normalized.startsWith('../') || normalized === '..') return false
  // Normalize must not produce an absolute-style path either.
  if (normalized.startsWith('/')) return false
  return true
}

// /v1/install bodies carry an optional lockfile + projects list; allow a
// generous ceiling but bound memory. /v1/files bodies only carry a list of
// digests, so the limit is smaller.
const INSTALL_BODY_MAX_BYTES = 64 * 1024 * 1024
const FILES_BODY_MAX_BYTES = 8 * 1024 * 1024

function readBody (req: http.IncomingMessage, maxBytes: number): Promise<string> {
  return new Promise<string>((resolve, reject) => {
    const chunks: Buffer[] = []
    let total = 0
    req.on('data', (chunk: Buffer) => {
      total += chunk.length
      if (total > maxBytes) {
        reject(Object.assign(new Error(`request body exceeds ${maxBytes} bytes`), { statusCode: 413 }))
        req.destroy()
        return
      }
      chunks.push(chunk)
    })
    req.on('end', () => resolve(Buffer.concat(chunks).toString('utf-8')))
    req.on('error', reject)
  })
}

