import { promises as fs } from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { PnpmError } from '@pnpm/error'
import gfs from '@pnpm/fs.graceful-fs'
import type { PackageInRegistry, PackageMeta } from '@pnpm/resolving.registry.types'
import { fastPathTemp as pathTemp } from 'path-temp'
import { renameOverwrite } from 'rename-overwrite'

import { markMetaCondensed, pickAbbreviatedVersionFields } from './clearMeta.js'

/**
 * Reading a lazily-hydrated version manifest throws this when its fragment
 * bytes are not valid JSON — the index vouched for the span, so the file is
 * corrupt. Callers treat the whole document like an unreadable mirror (a
 * cache miss), the same way whole-file corruption reads as `null` from
 * {@link loadMeta}; corruption discovered lazily must not degrade into
 * silently missing versions, or a poisoned mirror could keep 304-validating
 * forever (the etag lives in the intact headers record) and never self-heal.
 */
const MALFORMED_FRAGMENT_ERROR_CODE = 'ERR_PNPM_MALFORMED_META_FRAGMENT'

export function isMalformedMirrorFragmentError (err: unknown): boolean {
  return util.types.isNativeError(err) && 'code' in err && err.code === MALFORMED_FRAGMENT_ERROR_CODE
}

export interface MetaHeaders {
  etag?: string
  modified?: string
}

/**
 * Magic + format version of the indexed mirror layout, shared with the Rust
 * stack (`pnpm/crates/resolving-npm-resolver/src/mirror.rs`). The file starts
 * with `pacquet-meta-v1 <headersLen> <indexLen>\n`, followed by the headers
 * JSON record, the index JSON record, and the concatenated per-version JSON
 * fragments the index's `versions` spans point into. Loading such a file only
 * parses the two records; a version's manifest is parsed on first access.
 *
 * Files without the magic line are the two-line NDJSON layout (headers JSON,
 * newline, packument JSON), which loads eagerly. Both stacks read both
 * layouts, so mirrors written by either CLI version stay usable.
 */
const MIRROR_MAGIC = 'pacquet-meta-v1'

interface MirrorIndex {
  name: string
  distTags?: Record<string, string>
  time?: PackageMeta['time']
  versions: Array<[version: string, offset: number, length: number]>
}

/** Parse the `pacquet-meta-v1 <headersLen> <indexLen>` line; `null` for anything else. */
function parseMirrorMagic (line: string): { headersLen: number, indexLen: number } | null {
  if (!line.startsWith(`${MIRROR_MAGIC} `)) return null
  const [headersLen, indexLen, extra] = line.slice(MIRROR_MAGIC.length + 1).split(' ')
  if (extra != null) return null
  const parsedHeadersLen = Number.parseInt(headersLen, 10)
  const parsedIndexLen = Number.parseInt(indexLen, 10)
  if (!Number.isSafeInteger(parsedHeadersLen) || !Number.isSafeInteger(parsedIndexLen) || parsedHeadersLen < 0 || parsedIndexLen < 0) return null
  return { headersLen: parsedHeadersLen, indexLen: parsedIndexLen }
}

export interface LoadMetaOptions {
  /**
   * Condense hydrated version manifests to the abbreviated field set (see
   * `clearMeta`) and mark the returned document as already condensed. Only
   * affects indexed mirrors: an NDJSON mirror loads eagerly and the caller
   * condenses the whole document itself.
   */
  condense?: boolean
}

/**
 * Reads a package's mirrored registry metadata, in either mirror layout.
 * An indexed mirror yields a document whose `versions` values are parsed
 * lazily on first property access; enumeration of the version keys does not
 * parse any manifest. Returns `null` when the file is missing, malformed, or
 * truncated — the caller treats all three as a cache miss.
 */
export async function loadMeta (pkgMirror: string, opts?: LoadMetaOptions): Promise<PackageMeta | null> {
  let data: Buffer
  try {
    data = await gfs.readFile(pkgMirror)
  } catch {
    return null
  }
  try {
    const newlineIdx = data.indexOf(10)
    if (newlineIdx === -1) return null
    const firstLine = data.toString('utf8', 0, newlineIdx)
    const magic = parseMirrorMagic(firstLine)
    if (magic == null) {
      const headers = JSON.parse(firstLine) as MetaHeaders
      const meta = JSON.parse(data.toString('utf8', newlineIdx + 1)) as PackageMeta
      meta.etag = headers.etag
      return meta
    }
    const headersStart = newlineIdx + 1
    const indexStart = headersStart + magic.headersLen
    const fragmentBase = indexStart + magic.indexLen
    if (fragmentBase > data.length) return null
    const headers = JSON.parse(data.toString('utf8', headersStart, indexStart)) as MetaHeaders
    const index = JSON.parse(data.toString('utf8', indexStart, fragmentBase)) as MirrorIndex
    const versions = buildLazyVersions(pkgMirror, data, fragmentBase, index.versions, opts?.condense === true)
    if (versions == null) return null
    const meta: PackageMeta = {
      name: index.name,
      'dist-tags': index.distTags ?? {},
      versions,
      time: index.time,
      modified: headers.modified,
      etag: headers.etag,
    }
    if (opts?.condense === true) {
      markMetaCondensed(meta)
    }
    return meta
  } catch {
    return null
  }
}

/**
 * A versions map whose values parse from their fragment span on first access.
 * The getters cache in a map shared across the whole document, so a manifest
 * keeps one identity even when a property descriptor is copied onto a
 * filtered view of the document, and mutations of a hydrated manifest (the
 * picker's `name` back-fill, `readPackage` hooks) stick. Returns `null` when
 * a span falls outside the file, so a truncated mirror reads as a cache miss
 * rather than handing out garbage fragments later; a fragment whose bytes
 * don't parse throws on access (see the malformed-fragment error above).
 */
function buildLazyVersions (
  pkgMirror: string,
  data: Buffer,
  fragmentBase: number,
  spans: MirrorIndex['versions'],
  condense: boolean
): PackageMeta['versions'] | null {
  // A null prototype so a registry-controlled version key named `__proto__`
  // becomes a regular own property (see the same pattern in clearMeta).
  const versions: PackageMeta['versions'] = Object.create(null)
  const hydrated = new Map<string, PackageInRegistry | PnpmError>()
  for (const [version, offset, length] of spans) {
    if (!Number.isSafeInteger(offset) || !Number.isSafeInteger(length) || offset < 0 || length < 0) return null
    const start = fragmentBase + offset
    const end = start + length
    if (end > data.length) return null
    Object.defineProperty(versions, version, {
      enumerable: true,
      configurable: true,
      get (): PackageInRegistry {
        let manifest = hydrated.get(version)
        if (manifest == null) {
          try {
            const parsed = JSON.parse(data.toString('utf8', start, end)) as PackageInRegistry
            if (parsed == null || typeof parsed !== 'object') throw new Error('a version manifest must be an object')
            manifest = condense ? pickAbbreviatedVersionFields(parsed) : parsed
          } catch {
            manifest = new PnpmError('MALFORMED_META_FRAGMENT', `Failed to parse the manifest of ${version} in the package metadata mirror at ${pkgMirror}`)
          }
          hydrated.set(version, manifest)
        }
        if (manifest instanceof PnpmError) throw manifest
        return manifest
      },
    })
  }
  return versions
}

/**
 * Reads only the leading records of a mirror file to extract the cache
 * headers (etag, modified) for conditional requests. This avoids reading and
 * parsing the version data (which can be megabytes for popular packages).
 */
export async function loadMetaHeaders (pkgMirror: string): Promise<MetaHeaders | null> {
  let fh: fs.FileHandle | undefined
  try {
    fh = await fs.open(pkgMirror, 'r')
    // Magic line plus headers record is ~100 bytes; 1 KB is plenty.
    const buf = Buffer.alloc(1024)
    const { bytesRead } = await fh.read(buf, 0, 1024, 0)
    if (bytesRead === 0) return null
    const newlineIdx = buf.subarray(0, bytesRead).indexOf(10)
    if (newlineIdx === -1) return null
    const firstLine = buf.toString('utf8', 0, newlineIdx)
    const magic = parseMirrorMagic(firstLine)
    if (magic == null) {
      return JSON.parse(firstLine) as MetaHeaders
    }
    const headersStart = newlineIdx + 1
    const headersEnd = headersStart + magic.headersLen
    if (headersEnd <= bytesRead) {
      return JSON.parse(buf.toString('utf8', headersStart, headersEnd)) as MetaHeaders
    }
    const rest = Buffer.alloc(headersEnd - bytesRead)
    await fh.read(rest, 0, rest.length, bytesRead)
    return JSON.parse(buf.toString('utf8', headersStart, bytesRead) + rest.toString('utf8')) as MetaHeaders
  } catch {
    return null
  } finally {
    await fh?.close()
  }
}

/**
 * Formats metadata for disk storage as two-line NDJSON:
 *   Line 1: cache headers (etag, modified) — small, fast to read
 *   Line 2: the registry metadata JSON
 *
 * The etag lives only in the headers line (`loadMeta` re-attaches it from
 * there), so a `meta` that carries one is serialized without it. This layout
 * is kept for `filterMetadata` documents; everything else is mirrored in the
 * indexed layout (see {@link MIRROR_MAGIC}).
 */
export function prepareJsonForDisk (meta: PackageMeta, etag: string | undefined, jsonText?: string): string {
  const modified = meta.modified ?? meta.time?.modified
  const headers = JSON.stringify({ etag, modified })
  const body = jsonText ?? JSON.stringify(meta.etag == null ? meta : { ...meta, etag: undefined })
  return `${headers}\n${body}`
}

/**
 * Serializes metadata in the indexed mirror layout (see {@link MIRROR_MAGIC}).
 * Reading `meta.versions` here materializes a lazily-loaded document; the
 * documents that reach the mirror writers come from network fetches, which
 * are always eager.
 */
export function prepareIndexedForDisk (meta: PackageMeta, etag: string | undefined): Buffer {
  const headers = JSON.stringify({ etag, modified: meta.modified ?? meta.time?.modified } satisfies MetaHeaders)
  const spans: MirrorIndex['versions'] = []
  const fragments: Buffer[] = []
  let offset = 0
  for (const version of Object.keys(meta.versions ?? {})) {
    const manifest = meta.versions[version]
    if (manifest == null) continue
    const fragment = Buffer.from(JSON.stringify(manifest), 'utf8')
    spans.push([version, offset, fragment.length])
    offset += fragment.length
    fragments.push(fragment)
  }
  const index: MirrorIndex = {
    name: meta.name,
    distTags: meta['dist-tags'] ?? {},
    versions: spans,
  }
  if (meta.time != null) {
    index.time = meta.time
  }
  const indexJson = JSON.stringify(index)
  const lead = Buffer.from(
    `${MIRROR_MAGIC} ${Buffer.byteLength(headers)} ${Buffer.byteLength(indexJson)}\n${headers}${indexJson}`,
    'utf8'
  )
  return Buffer.concat([lead, ...fragments])
}

const createdDirs = new Set<string>()

export async function saveMeta (pkgMirror: string, content: string | Buffer): Promise<void> {
  const dir = path.dirname(pkgMirror)
  if (!createdDirs.has(dir)) {
    await fs.mkdir(dir, { recursive: true })
    createdDirs.add(dir)
  }
  const temp = pathTemp(pkgMirror)
  await gfs.writeFile(temp, content, typeof content === 'string' ? 'utf8' : undefined)
  await renameOverwrite(temp, pkgMirror)
}
