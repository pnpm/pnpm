import { promises as fs } from 'node:fs'
import path from 'node:path'

import { isError, PnpmError } from '@pnpm/error'
import gfs from '@pnpm/fs.graceful-fs'
import type { PackageInRegistry, PackageMeta } from '@pnpm/resolving.registry.types'
import { fastPathTemp as pathTemp } from 'path-temp'
import { renameOverwrite } from 'rename-overwrite'

import { markMetaCondensed, pickAbbreviatedVersionFields } from './clearMeta.js'
import { dropIncompletePublishTimes } from './publishTimes.js'

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
  return isError(err) && 'code' in err && err.code === MALFORMED_FRAGMENT_ERROR_CODE
}

/**
 * Whether `versions` holds a manifest for `version`, without invoking the
 * getter that parses a lazily-loaded manifest. A version listed with a
 * `null` value has no manifest.
 */
export function hasVersionManifest (versions: PackageMeta['versions'], version: string): boolean {
  const descriptor = Object.getOwnPropertyDescriptor(versions, version)
  return descriptor != null && (descriptor.get != null || descriptor.value != null)
}

export interface MetaHeaders {
  etag?: string
  fullEtag?: string
  modified?: string
  uncacheable?: boolean
}

/** What the headers record carries besides the document's own `modified`. */
export interface MirrorHeadersBody {
  uncacheable?: boolean
  fullEtag?: string
}

/**
 * Identifies the indexed mirror layout, shared with the Rust stack
 * (`pnpm/crates/resolving-npm-resolver/src/mirror.rs`). The file starts with
 * `pnpm-meta-v1 <headersLen> <indexLen>\n`, followed by the headers JSON
 * record, the index JSON record, and the concatenated per-version JSON
 * fragments the index's `versions` spans point into. Loading such a file only
 * parses the two records; a version's manifest is parsed on first access.
 *
 * A file whose first line isn't this is the two-line NDJSON layout (headers
 * JSON, newline, packument JSON), which loads eagerly — that is how a
 * `filterMetadata` document is still written. Reading an unrecognized line as
 * a cache miss is a backstop, not the compatibility story: a format change
 * bumps the cache directory (see `ABBREVIATED_META_DIR`) so a pnpm version
 * that predates it never opens these files at all.
 */
const MIRROR_FORMAT_ID = 'pnpm-meta-v1'

interface MirrorIndex {
  name: string
  distTags?: Record<string, string>
  time?: PackageMeta['time']
  /**
   * Not part of `PackageMeta`, so nothing on this side reads it back — it is
   * carried through because the Rust stack's `outdated --long` reads it out
   * of the shared mirror, and rewriting the file without it would blank that
   * column until the next full response.
   */
  homepage?: string
  versions: Array<[version: string, offset: number, length: number]>
}

/** Parse the `pnpm-meta-v1 <headersLen> <indexLen>` line; `null` for anything else. */
function parseFormatLine (line: string): { headersLen: number, indexLen: number } | null {
  if (!line.startsWith(`${MIRROR_FORMAT_ID} `)) return null
  const [headersLen, indexLen, extra] = line.slice(MIRROR_FORMAT_ID.length + 1).split(' ')
  if (extra != null) return null
  const parsedHeadersLen = parseRecordLength(headersLen)
  const parsedIndexLen = parseRecordLength(indexLen)
  if (parsedHeadersLen == null || parsedIndexLen == null) return null
  return { headersLen: parsedHeadersLen, indexLen: parsedIndexLen }
}

/** A whole token of decimal digits, as the Rust reader requires. */
function parseRecordLength (token: string | undefined): number | null {
  if (token == null || !/^\d+$/.test(token)) return null
  const length = Number(token)
  return Number.isSafeInteger(length) ? length : null
}

export interface LoadMetaOptions {
  /**
   * Condense hydrated version manifests to the abbreviated field set (see
   * `clearMeta`) and mark the returned document as already condensed. Only
   * affects indexed mirrors: an NDJSON mirror loads eagerly and the caller
   * condenses the whole document itself.
   */
  condense?: boolean
  /**
   * Parse every fragment before returning, so a corrupt one reads as a cache
   * miss here instead of throwing at the caller's first access. For callers
   * that read across all versions anyway and have no fall-through path of
   * their own, unlike the resolver (see the malformed-fragment error above).
   */
  hydrateEagerly?: boolean
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
    const format = parseFormatLine(firstLine)
    const meta = format == null
      ? parseNdjsonMeta(data, firstLine, newlineIdx)
      : parseIndexedMeta({ pkgMirror, data, recordsStart: newlineIdx + 1, format }, opts)
    if (meta != null) {
      dropIncompletePublishTimes(meta)
    }
    return meta
  } catch {
    return null
  }
}

function parseNdjsonMeta (data: Buffer, firstLine: string, newlineIdx: number): PackageMeta {
  const headers = JSON.parse(firstLine) as MetaHeaders
  const meta = JSON.parse(data.toString('utf8', newlineIdx + 1)) as PackageMeta
  return attachHeaders(meta, headers)
}

interface IndexedMirrorFile {
  pkgMirror: string
  data: Buffer
  recordsStart: number
  format: { headersLen: number, indexLen: number }
}

function parseIndexedMeta (
  { pkgMirror, data, recordsStart, format }: IndexedMirrorFile,
  opts?: LoadMetaOptions
): PackageMeta | null {
  const indexStart = recordsStart + format.headersLen
  const fragmentBase = indexStart + format.indexLen
  if (fragmentBase > data.length || format.headersLen > MAX_HEADERS_LEN || format.indexLen > MAX_INDEX_LEN) return null
  const headers = JSON.parse(data.toString('utf8', recordsStart, indexStart)) as MetaHeaders
  const index = JSON.parse(data.toString('utf8', indexStart, fragmentBase)) as MirrorIndex
  const versions = buildLazyVersions({ pkgMirror, data, fragmentBase }, index.versions, opts?.condense === true)
  if (versions == null) return null
  if (opts?.hydrateEagerly === true) {
    for (const version in versions) {
      void versions[version]
    }
  }
  const meta: PackageMeta = {
    name: index.name,
    'dist-tags': index.distTags ?? {},
    versions,
    time: index.time,
    modified: headers.modified,
  }
  if (opts?.condense === true) {
    markMetaCondensed(meta)
  }
  return attachHeaders(meta, headers)
}

function attachHeaders (meta: PackageMeta, headers: MetaHeaders): PackageMeta {
  meta.etag = headers.etag
  meta.fullEtag = headers.fullEtag
  meta.uncacheable = headers.uncacheable === true ? true : undefined
  return meta
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
  file: { pkgMirror: string, data: Buffer, fragmentBase: number },
  spans: MirrorIndex['versions'],
  condense: boolean
): PackageMeta['versions'] | null {
  // A null prototype so a registry-controlled version key named `__proto__`
  // becomes a regular own property (see the same pattern in clearMeta).
  const versions: PackageMeta['versions'] = Object.create(null)
  const hydrated = new Map<string, PackageInRegistry | PnpmError | null>()
  for (const [version, offset, length] of spans) {
    if (!Number.isSafeInteger(offset) || !Number.isSafeInteger(length) || offset < 0 || length < 0) return null
    const start = file.fragmentBase + offset
    const end = start + length
    if (end > file.data.length) return null
    // An oversized span is absent, as in the Rust reader.
    if (length > MAX_FRAGMENT_LEN) continue
    Object.defineProperty(versions, version, {
      enumerable: true,
      configurable: true,
      get (): PackageInRegistry {
        let manifest = hydrated.get(version)
        if (manifest === undefined) {
          manifest = parseFragment(file, { version, start, end }, condense)
          hydrated.set(version, manifest)
        }
        if (manifest instanceof PnpmError) throw manifest
        // `null` for a well-formed fragment of the wrong shape, which the
        // pickers treat as a version without a manifest.
        return manifest as PackageInRegistry
      },
    })
  }
  return versions
}

/** Matches the Rust reader's `MAX_FRAGMENT_LEN`. */
const MAX_FRAGMENT_LEN = 16 * 1024 * 1024

/**
 * Only bytes that are not UTF-8 JSON mean a damaged mirror. The mirror stores
 * registry fragments verbatim, so a well-formed fragment of the wrong shape is
 * how the registry served that version, and it reads as no manifest.
 */
function parseFragment (
  file: { pkgMirror: string, data: Buffer },
  { version, start, end }: { version: string, start: number, end: number },
  condense: boolean
): PackageInRegistry | PnpmError | null {
  let parsed: unknown
  try {
    parsed = JSON.parse(strictUtf8.decode(file.data.subarray(start, end)))
  } catch {
    return new PnpmError('MALFORMED_META_FRAGMENT', `Failed to parse the manifest of ${version} in the package metadata mirror at ${file.pkgMirror}`)
  }
  if (parsed == null || typeof parsed !== 'object' || Array.isArray(parsed)) return null
  return condense ? pickAbbreviatedVersionFields(parsed as PackageInRegistry) : parsed as PackageInRegistry
}

/** Fails on malformed UTF-8 rather than substituting U+FFFD, as the Rust reader does. */
const strictUtf8 = new TextDecoder('utf-8', { fatal: true })

/** Match the bounds the Rust reader applies to the two records. */
const MAX_HEADERS_LEN = 64 * 1024
const MAX_INDEX_LEN = 64 * 1024 * 1024

/**
 * Reads only the leading records of a mirror file to extract the cache
 * headers for conditional requests. This avoids reading and
 * parsing the version data (which can be megabytes for popular packages).
 */
export async function loadMetaHeaders (pkgMirror: string): Promise<MetaHeaders | null> {
  let fh: fs.FileHandle | undefined
  try {
    fh = await fs.open(pkgMirror, 'r')
    // Format line plus headers record is ~200 bytes; 1 KB is plenty.
    const buf = Buffer.alloc(1024)
    const { bytesRead } = await fh.read(buf, 0, 1024, 0)
    if (bytesRead === 0) return null
    const newlineIdx = buf.subarray(0, bytesRead).indexOf(10)
    if (newlineIdx === -1) return null
    const firstLine = buf.toString('utf8', 0, newlineIdx)
    const format = parseFormatLine(firstLine)
    if (format == null) {
      return JSON.parse(firstLine) as MetaHeaders
    }
    // The headers record is two etags and a timestamp. Bound the declared
    // length before allocating from it, so a corrupt or hostile mirror can't
    // turn a cache read into an arbitrarily large allocation.
    if (format.headersLen > MAX_HEADERS_LEN) return null
    const headersStart = newlineIdx + 1
    const headersEnd = headersStart + format.headersLen
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
 *   Line 1: cache headers (etag, fullEtag, modified) — small, fast to read
 *   Line 2: the registry metadata JSON
 *
 * This layout is kept for `filterMetadata` documents; everything else is
 * mirrored in the indexed layout (see {@link MIRROR_FORMAT_ID}).
 */
export function prepareJsonForDisk (
  meta: PackageMeta,
  etag: string | undefined,
  body: MirrorHeadersBody = {}
): string {
  const bodyMeta = meta.etag == null && meta.fullEtag == null && meta.uncacheable == null
    ? meta
    : { ...meta, etag: undefined, fullEtag: undefined, uncacheable: undefined }
  return `${encodeHeaders(meta, etag, body)}\n${JSON.stringify(bodyMeta)}`
}

/**
 * The headers record of either layout. The ETags live only here (`loadMeta`
 * re-attaches them), and `modified` is always written: it comes from the
 * packument's own `time.modified`, which both representations report
 * identically. An ETag identifies one representation: `etag` tags the
 * representation the mirror's directory holds, `body.fullEtag` a full document
 * the release-age upgrade stored in the abbreviated mirror. `body.uncacheable`
 * records that the response forbade caching, so the next online lookup
 * refetches instead of revalidating.
 */
function encodeHeaders (meta: PackageMeta, etag: string | undefined, body: MirrorHeadersBody): string {
  return JSON.stringify({
    etag,
    fullEtag: body.fullEtag,
    modified: meta.modified ?? meta.time?.modified,
    uncacheable: body.uncacheable === true ? true : undefined,
  } satisfies MetaHeaders)
}

/**
 * Serializes metadata in the indexed mirror layout (see {@link MIRROR_FORMAT_ID}).
 * Reading `meta.versions` here materializes a lazily-loaded document; the
 * documents that reach the mirror writers come from network fetches, which
 * are always eager.
 */
export function prepareIndexedForDisk (meta: PackageMeta, etag: string | undefined, body: MirrorHeadersBody = {}): Buffer {
  const headers = encodeHeaders(meta, etag, body)
  const spans: MirrorIndex['versions'] = []
  const fragments: Buffer[] = []
  let offset = 0
  for (const version of Object.keys(meta.versions ?? {})) {
    const manifest = meta.versions[version]
    if (manifest == null) continue
    const fragment = Buffer.from(JSON.stringify(manifest), 'utf8')
    // The reader skips such a span, so the saved and served views agree.
    if (fragment.length > MAX_FRAGMENT_LEN) continue
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
  const { homepage } = meta as PackageMeta & { homepage?: string }
  if (typeof homepage === 'string') {
    index.homepage = homepage
  }
  const indexJson = JSON.stringify(index)
  const lead = Buffer.from(
    `${MIRROR_FORMAT_ID} ${Buffer.byteLength(headers)} ${Buffer.byteLength(indexJson)}\n${headers}${indexJson}`,
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
