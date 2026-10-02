import crypto from 'node:crypto'
import fs from 'node:fs'
import fsPromises from 'node:fs/promises'
import { isIP } from 'node:net'
import path from 'node:path'
import { Transform } from 'node:stream'
import { pipeline } from 'node:stream/promises'

import { isError, PnpmError } from '@pnpm/error'
import type { BinaryFetcher, FetchFunction, FetchResult } from '@pnpm/fetching.fetcher-base'
import type { FetchFromRegistry, GetAuthHeader } from '@pnpm/fetching.types'
import type { StoreIndex } from '@pnpm/store.index'
import { addFilesFromDir } from '@pnpm/worker'
import { isSubdir } from 'is-subdir'
import { renameOverwrite } from 'rename-overwrite'
import ssri from 'ssri'
import yauzl from 'yauzl'

export interface CreateBinaryFetcherOptions {
  fetch: FetchFromRegistry
  fetchFromRemoteTarball: FetchFunction
  getAuthHeader?: GetAuthHeader
  storeIndex: StoreIndex
  offline?: boolean
  /**
   * Per-package-name regex sources (compatible with `new RegExp(pattern)`) matching file
   * paths inside the downloaded archive that should be skipped during extraction.
   * The lookup key is `pkg.name`. For zip archives, paths are matched relative to the
   * archive's top-level directory (i.e. after the `prefix` has been stripped).
   */
  archiveFilters?: Record<string, string>
}

export function createBinaryFetcher (ctx: CreateBinaryFetcherOptions): { binary: BinaryFetcher } {
  // Snapshot and pre-compile `archiveFilters` at creation time so later mutations to the
  // caller's object can't reintroduce invalid patterns, and so zip extraction doesn't
  // recompile the regex per fetch. The tarball path still needs the pattern string — it
  // crosses the worker thread boundary, where RegExp instances don't survive structured clone.
  const archiveFilters = compileArchiveFilters(ctx.archiveFilters)
  const fetchBinary: BinaryFetcher = async (cafs, resolution, opts) => {
    if (ctx.offline) {
      throw new PnpmError('CANNOT_DOWNLOAD_BINARY_OFFLINE', `Cannot download binary "${resolution.url}" because offline mode is enabled.`)
    }

    const manifest = {
      name: opts.pkg.name!,
      version: opts.pkg.version!,
      bin: resolution.bin,
    }
    const archiveFilter = opts.pkg.name != null ? archiveFilters.get(opts.pkg.name) : undefined
    const fetchResult = await fetchArchive(ctx, { cafs, resolution, opts, manifest, archiveFilter })
    return {
      ...fetchResult,
      manifest,
    }
  }
  return {
    binary: fetchBinary,
  }
}

interface CompiledArchiveFilter {
  pattern: string
  regex: RegExp
}

function compileArchiveFilters (archiveFilters: Record<string, string> | undefined): Map<string, CompiledArchiveFilter> {
  const compiled = new Map<string, CompiledArchiveFilter>()
  for (const [name, pattern] of Object.entries(archiveFilters ?? {})) {
    try {
      compiled.set(name, { pattern, regex: new RegExp(pattern) })
    } catch (err: unknown) {
      const detail = isError(err) ? `: ${err.message}` : ''
      throw new PnpmError(
        'INVALID_ARCHIVE_FILTER',
        `Invalid archive filter regex for "${name}"${detail}: ${pattern}`
      )
    }
  }
  return compiled
}

type BinaryFetcherArgs = Parameters<BinaryFetcher>

interface FetchArchiveParams {
  cafs: BinaryFetcherArgs[0]
  resolution: BinaryFetcherArgs[1]
  opts: BinaryFetcherArgs[2]
  manifest: { name: string, version: string, bin: BinaryFetcherArgs[1]['bin'] }
  archiveFilter: CompiledArchiveFilter | undefined
}

async function fetchArchive (ctx: CreateBinaryFetcherOptions, params: FetchArchiveParams): Promise<FetchResult> {
  const { cafs, resolution, opts, manifest, archiveFilter } = params
  switch (resolution.archive) {
    case 'tarball': {
      return ctx.fetchFromRemoteTarball(cafs, {
        tarball: resolution.url,
        integrity: resolution.integrity,
      }, {
        ...opts,
        appendManifest: manifest,
        ignoreFilePattern: archiveFilter?.pattern ?? opts.ignoreFilePattern,
      })
    }
    case 'zip': {
      return fetchZipArchive(ctx, params)
    }
    default: {
      throw new PnpmError('NOT_SUPPORTED_ARCHIVE', `The binary fetcher doesn't support archive type ${resolution.archive as string}`)
    }
  }
}

async function fetchZipArchive (
  ctx: CreateBinaryFetcherOptions,
  { cafs, resolution, opts, manifest, archiveFilter }: FetchArchiveParams
): Promise<FetchResult> {
  const tempLocation = await cafs.tempDir()
  await downloadAndUnpackZip(ctx.fetch, {
    url: resolution.url,
    integrity: resolution.integrity,
    basename: resolution.prefix ?? '',
    authHeaderValue: getSecureNodeMirrorAuthHeader(ctx.getAuthHeader, resolution.url, opts.pkg.name),
    ignoreEntry: archiveFilter?.regex,
  }, tempLocation)
  return addFilesFromDir({
    storeDir: cafs.storeDir,
    storeIndex: ctx.storeIndex,
    dir: tempLocation,
    filesIndexFile: opts.filesIndexFile,
    readManifest: false,
    appendManifest: manifest,
    includeNodeModules: true,
  })
}

function getSecureNodeMirrorAuthHeader (
  getAuthHeader: GetAuthHeader | undefined,
  url: string,
  packageName: string | undefined
): string | undefined {
  const authHeaderValue = getAuthHeader?.(url)
  if (authHeaderValue == null || packageName !== 'node') return authHeaderValue
  const parsed = new URL(url)
  if (parsed.protocol === 'https:' || isLoopbackHost(parsed.hostname)) return authHeaderValue
  return undefined
}

function isLoopbackHost (hostname: string): boolean {
  return hostname === 'localhost' || hostname === '[::1]' || (isIP(hostname) === 4 && hostname.startsWith('127.'))
}

export interface AssetInfo {
  url: string
  integrity: string
  basename: string
  authHeaderValue?: string
  /**
   * Regex matched against each zip entry's path relative to the archive's top-level basename.
   * Matching entries are skipped during extraction.
   */
  ignoreEntry?: RegExp
}

/**
 * Downloads and unpacks a zip file containing a binary asset.
 *
 * @throws {PnpmError} When integrity verification fails or extraction fails
 */
export async function downloadAndUnpackZip (
  fetchFromRegistry: FetchFromRegistry,
  assetInfo: AssetInfo,
  targetDir: string
): Promise<void> {
  // tempy resolves os.tmpdir() when loaded, which throws if that directory is missing.
  const { temporaryDirectory } = await import('tempy')
  const tmp = path.join(temporaryDirectory(), 'pnpm.zip')

  try {
    await downloadWithIntegrityCheck(fetchFromRegistry, assetInfo, tmp)
    await extractZipToTarget(tmp, assetInfo.basename, targetDir, assetInfo.ignoreEntry)
  } finally {
    try {
      await fsPromises.unlink(tmp)
    } catch {
      // Ignore cleanup errors
    }
  }
}

/**
 * Streams a download to `tmpPath`, verifying its integrity on the way.
 */
async function downloadWithIntegrityCheck (
  fetchFromRegistry: FetchFromRegistry,
  { url, integrity, authHeaderValue }: AssetInfo,
  tmpPath: string
): Promise<void> {
  const response = await fetchFromRegistry(url, { authHeaderValue })
  const expected = ssri.parse(integrity)
  const algorithm = expected.pickAlgorithm()
  const hash = crypto.createHash(algorithm)
  await pipeline(
    response.body as AsyncIterable<Uint8Array>,
    new Transform({
      transform (chunk: Buffer, _encoding, callback) {
        hash.update(chunk)
        callback(null, chunk)
      },
    }),
    fs.createWriteStream(tmpPath)
  )
  const found = ssri.fromHex(hash.digest('hex'), algorithm)
  if (!expected.match(found)) {
    throw new PnpmError('TARBALL_INTEGRITY', `Got unexpected checksum for "${url}". Wanted "${expected.toString()}". Got "${found.toString()}".`)
  }
}

/**
 * Extracts a zip file to the target directory.
 *
 * @throws {PnpmError} When extraction fails or path traversal is detected
 */
async function extractZipToTarget (
  zipPath: string,
  basename: string,
  targetDir: string,
  ignoreEntry?: RegExp
): Promise<void> {
  const extractionRoot = basename === ''
    ? targetDir
    : await fsPromises.mkdtemp(path.join(path.dirname(targetDir), '_unzip_'))

  try {
    if (basename !== '') {
      validatePathSecurity(extractionRoot, basename)
    }
    await extractEntries(zipPath, { extractionRoot, basename, ignoreEntry })
    await renameOverwrite(path.join(extractionRoot, basename), targetDir)
  } finally {
    if (extractionRoot !== targetDir) {
      await fsPromises.rm(extractionRoot, { recursive: true, force: true })
    }
  }
}

interface ExtractEntriesOptions {
  extractionRoot: string
  basename: string
  ignoreEntry?: RegExp
}

/**
 * Extracts one entry at a time, streaming each to disk. An entry is read only up
 * to the uncompressed size the archive declares for it.
 */
async function extractEntries (zipPath: string, { extractionRoot, basename, ignoreEntry }: ExtractEntriesOptions): Promise<void> {
  const basenamePrefix = basename === '' ? '' : `${basename}/`
  const testEntry = toStatelessTester(ignoreEntry)

  await fsPromises.mkdir(extractionRoot, { recursive: true })
  const createdDirs = new Set<string>()
  // File names are decoded here rather than by yauzl so that an unsafe path is
  // rejected by validatePathSecurity, with its error code.
  const zipfile = await yauzl.openPromise(zipPath, { decodeStrings: false })
  try {
    for await (const entry of zipfile.eachEntry()) {
      const entryPath = yauzl.getFileNameLowLevel(entry.generalPurposeBitFlag, entry.fileNameRaw, entry.extraFields, false)
      // Directory entries are optional in a zip, so directories are created from file paths instead.
      if (entryPath.endsWith('/')) continue
      validatePathSecurity(extractionRoot, entryPath)
      if (testEntry?.(stripBasenamePrefix(entryPath, basenamePrefix))) continue
      await mkdirWithoutFollowingSymlinks(extractionRoot, path.dirname(entryPath), createdDirs)
      await extractEntry(zipfile, entry, path.join(extractionRoot, entryPath))
    }
  } finally {
    zipfile.close()
  }
}

function stripBasenamePrefix (entryPath: string, basenamePrefix: string): string {
  return basenamePrefix && entryPath.startsWith(basenamePrefix)
    ? entryPath.slice(basenamePrefix.length)
    : entryPath
}

/**
 * Creates `relativeDir` under `root` one segment at a time, refusing any segment
 * that already exists as something other than a directory. A symlink or junction
 * there could otherwise lead the extraction outside of `root`.
 */
async function mkdirWithoutFollowingSymlinks (root: string, relativeDir: string, createdDirs: Set<string>): Promise<void> {
  let dir = root
  for (const segment of relativeDir.split('/')) {
    if (segment === '' || segment === '.') continue
    dir = path.join(dir, segment)
    if (createdDirs.has(dir)) continue
    await mkdirOrAssertRealDirectory(dir) // eslint-disable-line no-await-in-loop -- each segment is created inside the previous one, once the previous one is known to be a real directory
    createdDirs.add(dir)
  }
}

async function mkdirOrAssertRealDirectory (dir: string): Promise<void> {
  try {
    await fsPromises.mkdir(dir)
  } catch (err: unknown) {
    if (!(isError(err) && 'code' in err && err.code === 'EEXIST')) throw err
    if (!(await fsPromises.lstat(dir)).isDirectory()) {
      throw new PnpmError('PATH_TRAVERSAL', `Refusing to extract into "${dir}" because it is not a directory`)
    }
  }
}

async function extractEntry (zipfile: yauzl.ZipFile, entry: yauzl.Entry, target: string): Promise<void> {
  // A later entry with the same path replaces an earlier one. The file is
  // removed rather than opened for writing, so that a symlink at the path is
  // replaced instead of followed.
  await fsPromises.rm(target, { force: true })
  await pipeline(
    await zipfile.openReadStreamPromise(entry),
    fs.createWriteStream(target, { flags: 'wx' })
  )
}

function toStatelessTester (regex: RegExp | undefined): ((input: string) => boolean) | undefined {
  if (!regex) return undefined
  // `/g` and `/y` make `RegExp.prototype.test` stateful via `lastIndex`.
  if (!regex.global && !regex.sticky) {
    return (input) => regex.test(input)
  }
  const safeFlags = regex.flags.replace(/[gy]/g, '')
  const clone = new RegExp(regex.source, safeFlags)
  return (input) => clone.test(input)
}

/**
 * Validates that a path does not escape the base directory via path traversal.
 *
 * @throws {PnpmError} When path traversal is detected
 */
function validatePathSecurity (basePath: string, targetPath: string): void {
  if (path.isAbsolute(targetPath)) {
    throw new PnpmError('PATH_TRAVERSAL',
      `Refusing to extract path "${targetPath}" - absolute paths are not allowed`)
  }
  const normalizedTarget = path.resolve(basePath, targetPath)
  if (!isSubdir(basePath, normalizedTarget) && normalizedTarget !== basePath) {
    throw new PnpmError('PATH_TRAVERSAL',
      `Refusing to extract path "${targetPath}" outside of target directory`)
  }
}
