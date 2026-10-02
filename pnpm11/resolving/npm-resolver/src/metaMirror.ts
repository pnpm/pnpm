import { promises as fs } from 'node:fs'
import path from 'node:path'

import { createHexHash } from '@pnpm/crypto.hash'
import gfs from '@pnpm/fs.graceful-fs'
import { logger } from '@pnpm/logger'
import type { PackageMeta } from '@pnpm/resolving.registry.types'
import pLimit, { type LimitFunction } from 'p-limit'
import { fastPathTemp as pathTemp } from 'path-temp'
import { renameOverwrite } from 'rename-overwrite'

import { clearMeta, retainsFullMeta } from './clearMeta.js'
import { encodeRegistry } from './encodeRegistry.js'
import { dropIncompletePublishTimes } from './publishTimes.js'

interface RefCountedLimiter {
  count: number
  limit: LimitFunction
}

/**
 * prevents simultaneous operations on the meta.json
 * otherwise it would cause EPERM exceptions
 */
const metafileOperationLimits = {} as {
  [pkgMirror: string]: RefCountedLimiter | undefined
}

/**
 * To prevent metafileOperationLimits from holding onto objects in memory on
 * the order of the number of packages, refcount the limiters and drop them
 * once they are no longer needed. Callers of this function should ensure
 * that the limiter is no longer referenced once fn's Promise has resolved.
 */
export async function runLimited<Result> (pkgMirror: string, fn: (limit: LimitFunction) => Promise<Result>): Promise<Result> {
  let entry!: RefCountedLimiter
  try {
    entry = metafileOperationLimits[pkgMirror] ??= { count: 0, limit: pLimit(1) }
    entry.count++
    return await fn(entry.limit)
  } finally {
    entry.count--
    if (entry.count === 0) {
      metafileOperationLimits[pkgMirror] = undefined
    }
  }
}

/**
 * The form in which a packument is retained in memory (see {@link clearMeta}
 * for why). Full documents reach even a plain install via optional
 * dependencies (fetched full for `libc`), release-age `time` upgrades, and
 * mirror files that hold a full body.
 */
export function condenseMetaForCache (
  ctx: { fullMetadata?: boolean, filterMetadata?: boolean },
  meta: PackageMeta
): PackageMeta {
  return retainsFullMeta(ctx) ? meta : clearMeta(meta)
}

/**
 * The mirror is an optimization, so a write failure only gets a debug log
 * with the mirror path and the install continues.
 */
export function saveMetaBestEffort (pkgMirror: string, json: string, uncacheable = false): void {
  void runLimited(pkgMirror, (limit) => limit(async () => {
    try {
      await saveMeta(pkgMirror, json)
    } catch (err: unknown) {
      logger.debug({ message: `Failed to write the package metadata mirror at ${pkgMirror}`, err })
      await discardMirrorAfterFailedUncacheableWrite(pkgMirror, uncacheable)
    }
  }))
}

/**
 * A failed uncacheable write leaves the previous header in place. The next
 * fetch would send that header's validators and can accept a stale 304.
 */
export async function discardMirrorAfterFailedUncacheableWrite (pkgMirror: string, uncacheable: boolean): Promise<void> {
  if (!uncacheable) return
  await fs.rm(pkgMirror, { force: true }).catch(() => undefined)
}

export function encodePkgName (pkgName: string): string {
  if (pkgName !== pkgName.toLowerCase()) {
    return `${pkgName}_${createHexHash(pkgName)}`
  }
  return pkgName
}

/**
 * Key for the in-memory `metaCache` holding a package's registry metadata. The
 * registry is part of the key so that a package of the same name served by two
 * registries in one install can't collide on a single slot (which would resolve
 * the wrong tarball/integrity). `fullMetadata` and `filterMetadata` keep the
 * abbreviated, full, and filtered-full documents in distinct slots, mirroring
 * the on-disk `metaDir` split: a `filterMetadata` resolver stores a `clearMeta`-
 * stripped packument, so it must not share a slot with an unfiltered full one
 * (reachable only when a `metaCache` is shared across resolvers with different
 * settings). `filterMetadata` only narrows the full slot — abbreviated metadata
 * shares one on-disk mirror regardless, so its key carries no filtered variant.
 * `\x00` can't appear in a registry URL or a package name, so it's an
 * unambiguous separator. The verifier reads this same cache and must build the
 * key with this function.
 *
 * The registry is canonicalized to its origin plus a trailing-slashed path, so
 * the resolver (which may pass a configured named-registry URL verbatim) and
 * the verifier (which routes through trailing-slashed prefixes) converge on one
 * key for the same logical registry instead of creating duplicate slots. Origin
 * and path are preserved, so two registries that genuinely differ never collapse.
 */
export function getPkgMetaCacheKey (registry: string, pkgName: string, fullMetadata: boolean, filterMetadata: boolean): string {
  const key = `${canonicalizeRegistry(registry)}\x00${pkgName}`
  if (!fullMetadata) return key
  return filterMetadata ? `${key}:full:filtered` : `${key}:full`
}

function canonicalizeRegistry (registry: string): string {
  try {
    const parsed = new URL(registry)
    const pathname = parsed.pathname.endsWith('/') ? parsed.pathname : `${parsed.pathname}/`
    return `${parsed.origin}${pathname}`
  } catch {
    return registry
  }
}

/**
 * Path of the on-disk JSONL document where pnpm mirrors a package's registry
 * metadata. `metaDir` selects between abbreviated and full caches.
 */
export function getPkgMirrorPath (cacheDir: string, metaDir: string, registry: string, pkgName: string): string {
  return path.join(cacheDir, metaDir, encodeRegistry(registry), `${encodePkgName(pkgName)}.jsonl`)
}

/**
 * Hint for `NO_OFFLINE_META`: whether the package's metadata sits on disk
 * under the legacy mirror path, which this pnpm version no longer reads.
 * `undefined` when no such mirror exists, so the base error message stands
 * on its own.
 */
export async function legacyMirrorHint (cacheDir: string, metaDir: string, registry: string, pkgName: string): Promise<string | undefined> {
  const legacyMirror = getLegacyPkgMirrorPath(cacheDir, metaDir, registry, pkgName)
  if (legacyMirror == null) return undefined
  try {
    await fs.access(legacyMirror)
  } catch {
    return undefined
  }
  return `The cache layout for registry metadata changed in pnpm 11.27 and 12.4. ${legacyMirror} holds a mirror ` +
    'from an older pnpm version, which this offline install cannot read. Run one online install to repopulate ' +
    'the cache under the new layout, then retry offline.'
}

/**
 * The legacy mirror path for a registry: `<host>[:<port>]` with `:`
 * replaced by `+`, and no scheme, path segments, or hash suffix.
 * `null` for a registry URL {@link getPkgMirrorPath} would itself reject.
 */
function getLegacyPkgMirrorPath (cacheDir: string, metaDir: string, registry: string, pkgName: string): string | null {
  let url: URL
  try {
    url = new URL(registry)
  } catch {
    return null
  }
  if (url.host === '') return null
  return path.join(cacheDir, metaDir, url.host.replace(':', '+'), `${encodePkgName(pkgName)}.jsonl`)
}

/**
 * Formats metadata for disk storage as two-line NDJSON:
 *   Line 1: cache headers (etag, fullEtag, modified) — small, fast to read
 *   Line 2: the registry metadata JSON
 *
 * The ETags live only in the headers line (`loadMeta` re-attaches them from
 * there), so a `meta` that carries one is serialized without it.
 *
 * An ETag identifies one representation. `etag` tags the representation the
 * mirror's directory holds. `body.fullEtag` tags a full document that the
 * release-age upgrade stored in the abbreviated mirror (see
 * {@link mirrorEtags}). `modified` is always written: it comes from the
 * packument's own `time.modified`, which both representations report
 * identically.
 *
 * `body.jsonText` is the raw registry body, written as is when given.
 * `body.uncacheable` records that the response forbade caching, so the next
 * online lookup refetches instead of revalidating.
 */
export function prepareJsonForDisk (
  meta: PackageMeta,
  etag: string | undefined,
  body: { jsonText?: string, uncacheable?: boolean, fullEtag?: string } = {}
): string {
  const modified = meta.modified ?? meta.time?.modified
  const headers = JSON.stringify({
    etag,
    fullEtag: body.fullEtag,
    modified,
    uncacheable: body.uncacheable === true ? true : undefined,
  })
  const bodyMeta = meta.etag == null && meta.fullEtag == null && meta.uncacheable == null
    ? meta
    : { ...meta, etag: undefined, fullEtag: undefined, uncacheable: undefined }
  return `${headers}\n${body.jsonText ?? JSON.stringify(bodyMeta)}`
}

/**
 * Where a response's ETag goes in a mirror's headers. A full document stored
 * in the abbreviated mirror keeps its ETag as `fullEtag`, because that tag
 * validates only the full representation.
 */
export function mirrorEtags (
  response: { etag?: string, fullMetadata?: boolean },
  mirrorFullMetadata: boolean
): Pick<MetaHeaders, 'etag' | 'fullEtag'> {
  return holdsFullMetaInAbbreviatedMirror(response, mirrorFullMetadata)
    ? { fullEtag: response.etag }
    : { etag: response.etag }
}

/**
 * Whether a response is a full document headed for the abbreviated mirror.
 * Only `time` is needed from it there, so it is mirrored condensed.
 */
export function holdsFullMetaInAbbreviatedMirror (
  response: { fullMetadata?: boolean },
  mirrorFullMetadata: boolean
): boolean {
  return response.fullMetadata === true && !mirrorFullMetadata
}

/**
 * The full document's ETag when the abbreviated mirror holds a document the
 * release-age upgrade stored there. That mirror is revalidated as the full
 * document with this tag, so a registry with per-representation ETags, such
 * as npmjs.org, can answer 304.
 */
export function fullEtagOfAbbreviatedMirror (headers: MetaHeaders | null, fullMetadata: boolean): string | undefined {
  if (fullMetadata || headers?.fullEtag == null || headers.fullEtag === '') return undefined
  return headers.fullEtag
}

export async function getFileMtime (filePath: string): Promise<Date | null> {
  try {
    const stat = await fs.stat(filePath)
    return stat.mtime
  } catch {
    return null
  }
}

export interface MetaHeaders {
  etag?: string
  fullEtag?: string
  modified?: string
  uncacheable?: boolean
}

export function metaHeadersOf (meta: PackageMeta): MetaHeaders {
  return {
    etag: meta.etag,
    fullEtag: meta.fullEtag,
    modified: meta.modified ?? meta.time?.modified,
    uncacheable: meta.uncacheable,
  }
}

/**
 * Reads only the first line of the cached NDJSON metadata file to extract
 * the cache headers (etag, modified). This avoids reading and
 * parsing the full metadata (which can be megabytes for popular packages)
 * when we only need conditional-request headers.
 */
export async function loadMetaHeaders (pkgMirror: string): Promise<MetaHeaders | null> {
  let fh: fs.FileHandle | undefined
  try {
    fh = await fs.open(pkgMirror, 'r')
    // The first line (headers JSON) is typically ~100 bytes; 1 KB is plenty.
    const buf = Buffer.alloc(1024)
    const { bytesRead } = await fh.read(buf, 0, 1024, 0)
    if (bytesRead === 0) return null
    const chunk = buf.toString('utf8', 0, bytesRead)
    const newlineIdx = chunk.indexOf('\n')
    if (newlineIdx === -1) return null
    return JSON.parse(chunk.slice(0, newlineIdx)) as MetaHeaders
  } catch {
    return null
  } finally {
    await fh?.close()
  }
}

/**
 * Reads the full metadata from the cached NDJSON file.
 * Line 1: cache headers (etag, modified)
 * Line 2: registry metadata JSON
 */
export async function loadMeta (pkgMirror: string): Promise<PackageMeta | null> {
  try {
    const data = await gfs.readFile(pkgMirror, 'utf8')
    const newlineIdx = data.indexOf('\n')
    if (newlineIdx === -1) return null
    const headers = JSON.parse(data.slice(0, newlineIdx)) as MetaHeaders
    const meta = JSON.parse(data.slice(newlineIdx + 1)) as PackageMeta
    dropIncompletePublishTimes(meta)
    meta.etag = headers.etag
    meta.fullEtag = headers.fullEtag
    meta.uncacheable = headers.uncacheable === true ? true : undefined
    return meta
  } catch {
    return null
  }
}

const createdDirs = new Set<string>()

export async function saveMeta (pkgMirror: string, json: string): Promise<void> {
  const dir = path.dirname(pkgMirror)
  if (!createdDirs.has(dir)) {
    await fs.mkdir(dir, { recursive: true })
    createdDirs.add(dir)
  }
  const temp = pathTemp(pkgMirror)
  await gfs.writeFile(temp, json, 'utf8')
  await renameOverwrite(temp, pkgMirror)
}
