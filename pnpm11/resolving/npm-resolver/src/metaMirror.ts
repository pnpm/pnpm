import { promises as fs } from 'node:fs'
import path from 'node:path'

import { createHexHash } from '@pnpm/crypto.hash'
import { logger } from '@pnpm/logger'
import type { PackageMeta } from '@pnpm/resolving.registry.types'
import pLimit, { type LimitFunction } from 'p-limit'

import { clearMeta, retainsFullMeta } from './clearMeta.js'
import { encodeRegistry } from './encodeRegistry.js'
import type { FetchMetadataResult } from './fetch.js'
import {
  type MetaHeaders,
  type MirrorHeadersBody,
  prepareIndexedForDisk,
  prepareJsonForDisk,
  saveMeta,
} from './mirrorLayout.js'

export {
  hasVersionManifest,
  isMalformedMirrorFragmentError,
  loadMeta,
  loadMetaHeaders,
  type LoadMetaOptions,
  type MetaHeaders,
  prepareIndexedForDisk,
  prepareJsonForDisk,
  saveMeta,
} from './mirrorLayout.js'

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
export function saveMetaBestEffort (pkgMirror: string, content: string | Buffer, uncacheable = false): void {
  void runLimited(pkgMirror, (limit) => limit(async () => {
    try {
      await saveMeta(pkgMirror, content)
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

/**
 * Every project of a workspace that joins one in-flight fetch mirrors its
 * result, so the encoded form is memoized on the result object. Encoding it
 * per project would hold as many copies of a body reaching tens of MB as
 * there are projects. Keyed on the result rather than its `meta`, so the copy
 * is released with the shared body: `memoizeFetchMetadata` drops the result
 * once the request settles.
 */
const encodedMirrors = new WeakMap<FetchMetadataResult, Map<string, string | Buffer>>()

/**
 * The mirror form of `meta`, a document fetched as `result`. A
 * `filterMetadata` resolver mirrors the stripped NDJSON form, because that
 * mirror only serves equally stripped resolutions. Everything else uses the
 * indexed layout, whose per-version spans later loads hydrate lazily.
 */
export function encodeMirror (
  ctx: { filterMetadata?: boolean },
  result: FetchMetadataResult,
  { meta, etag, body }: { meta: PackageMeta, etag: string | undefined, body: MirrorHeadersBody }
): string | Buffer {
  let encoded = encodedMirrors.get(result)
  if (encoded == null) {
    encoded = new Map()
    encodedMirrors.set(result, encoded)
  }
  const key = JSON.stringify([ctx.filterMetadata === true, meta === result.meta, etag, body.fullEtag, body.uncacheable === true])
  let content = encoded.get(key)
  if (content == null) {
    content = ctx.filterMetadata === true
      ? prepareJsonForDisk(meta, etag, body)
      : prepareIndexedForDisk(meta, etag, body)
    encoded.set(key, content)
  }
  return content
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
 * under a legacy mirror path, which this pnpm version no longer reads.
 * `undefined` when no such mirror exists, so the base error message stands
 * on its own.
 */
export async function legacyMirrorHint (cacheDir: string, metaDir: string, registry: string, pkgName: string): Promise<string | undefined> {
  for (const legacyMirror of getLegacyPkgMirrorPaths(cacheDir, metaDir, registry, pkgName)) {
    try {
      // eslint-disable-next-line no-await-in-loop -- the candidates are probed in order and the first hit wins
      await fs.access(legacyMirror)
    } catch {
      continue
    }
    return `The cache layout for registry metadata has changed. ${legacyMirror} holds a mirror ` +
      'from an older pnpm version, which this offline install cannot read. Run one online install to repopulate ' +
      'the cache under the new layout, then retry offline.'
  }
  return undefined
}

/**
 * The version directory of the NDJSON-era cache layout. The indexed layout
 * moved every mirror to a new version directory, so nothing reads it.
 */
const LEGACY_META_VERSION_DIR = 'v11'

/**
 * Where an older pnpm may have mirrored the package for `metaDir`: the
 * legacy version directory, under the current registry key and under the
 * legacy one (`<host>[:<port>]` with `:` replaced by `+`, and no scheme, path
 * segments, or hash suffix).
 */
function getLegacyPkgMirrorPaths (cacheDir: string, metaDir: string, registry: string, pkgName: string): string[] {
  const legacyMetaDir = path.join(LEGACY_META_VERSION_DIR, path.basename(metaDir))
  const fileName = `${encodePkgName(pkgName)}.jsonl`
  const paths = [path.join(cacheDir, legacyMetaDir, encodeRegistry(registry), fileName)]
  let url: URL
  try {
    url = new URL(registry)
  } catch {
    return paths
  }
  if (url.host !== '') {
    paths.push(path.join(cacheDir, legacyMetaDir, url.host.replace(':', '+'), fileName))
  }
  return paths
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

export function metaHeadersOf (meta: PackageMeta): MetaHeaders {
  return {
    etag: meta.etag,
    fullEtag: meta.fullEtag,
    modified: meta.modified ?? meta.time?.modified,
    uncacheable: meta.uncacheable,
  }
}
