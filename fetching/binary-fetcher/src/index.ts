import fsPromises from 'node:fs/promises'
import path from 'node:path'
import util from 'node:util'

import { PnpmError } from '@pnpm/error'
import type { BinaryFetcher, FetchFunction, FetchResult } from '@pnpm/fetching.fetcher-base'
import type { FetchFromRegistry } from '@pnpm/fetching.types'
import type { StoreIndex } from '@pnpm/store.index'
import { addFilesFromDir } from '@pnpm/worker'
import AdmZip from 'adm-zip'
import { isSubdir } from 'is-subdir'
import { renameOverwrite } from 'rename-overwrite'
import ssri from 'ssri'
import { temporaryDirectory } from 'tempy'

export interface CreateBinaryFetcherOptions {
  fetch: FetchFromRegistry
  fetchFromRemoteTarball: FetchFunction
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
  const archiveFilters = new Map<string, { pattern: string, regex: RegExp }>()
  for (const [name, pattern] of Object.entries(ctx.archiveFilters ?? {})) {
    try {
      archiveFilters.set(name, { pattern, regex: new RegExp(pattern) })
    } catch (err: unknown) {
      const detail = util.types.isNativeError(err) ? `: ${err.message}` : ''
      throw new PnpmError(
        'INVALID_ARCHIVE_FILTER',
        `Invalid archive filter regex for "${name}"${detail}: ${pattern}`
      )
    }
  }
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

    let fetchResult!: FetchResult
    switch (resolution.archive) {
      case 'tarball': {
        fetchResult = await ctx.fetchFromRemoteTarball(cafs, {
          tarball: resolution.url,
          integrity: resolution.integrity,
        }, {
          ...opts,
          appendManifest: manifest,
          ignoreFilePattern: archiveFilter?.pattern ?? opts.ignoreFilePattern,
        })
        break
      }
      case 'zip': {
        const tempLocation = await cafs.tempDir()
        await downloadAndUnpackZip(ctx.fetch, {
          url: resolution.url,
          integrity: resolution.integrity,
          basename: resolution.prefix ?? '',
          ignoreEntry: archiveFilter?.regex,
        }, tempLocation)
        fetchResult = await addFilesFromDir({
          storeDir: cafs.storeDir,
          storeIndex: ctx.storeIndex,
          dir: tempLocation,
          filesIndexFile: opts.filesIndexFile,
          readManifest: false,
          appendManifest: manifest,
          includeNodeModules: true,
        })
        break
      }
      default: {
        throw new PnpmError('NOT_SUPPORTED_ARCHIVE', `The binary fetcher doesn't support archive type ${resolution.archive as string}`)
      }
    }
    return {
      ...fetchResult,
      manifest,
    }
  }
  return {
    binary: fetchBinary,
  }
}

export interface AssetInfo {
  url: string
  integrity: string
  basename: string
  /**
   * Regex matched against each zip entry's path relative to the archive's top-level basename.
   * Matching entries are skipped during extraction.
   */
  ignoreEntry?: RegExp
}

/**
 * Downloads and unpacks a zip file containing a binary asset.
 *
 * @param fetchFromRegistry - Function to fetch resources from registry
 * @param assetInfo - Information about the binary asset
 * @param targetDir - Directory where the binary asset should be installed
 * @throws {PnpmError} When integrity verification fails or extraction fails
 */
export async function downloadAndUnpackZip (
  fetchFromRegistry: FetchFromRegistry,
  assetInfo: AssetInfo,
  targetDir: string
): Promise<void> {
  const tmp = path.join(temporaryDirectory(), 'pnpm.zip')

  try {
    await downloadWithIntegrityCheck(fetchFromRegistry, assetInfo, tmp)
    await extractZipToTarget(tmp, assetInfo.basename, targetDir, assetInfo.ignoreEntry)
  } finally {
    // Clean up temporary file
    try {
      await fsPromises.unlink(tmp)
    } catch {
      // Ignore cleanup errors
    }
  }
}

/**
 * Downloads a file with integrity verification.
 */
async function downloadWithIntegrityCheck (
  fetchFromRegistry: FetchFromRegistry,
  { url, integrity }: AssetInfo,
  tmpPath: string
): Promise<void> {
  const response = await fetchFromRegistry(url)

  // Collect all chunks from the response
  const chunks: Buffer[] = []
  for await (const chunk of response.body!) {
    chunks.push(chunk as Buffer)
  }
  const data = Buffer.concat(chunks)

  try {
    // Verify integrity if provided
    ssri.checkData(data, integrity, { error: true })
  } catch (err) {
    if (!(err instanceof Error) || !('expected' in err) || !('found' in err)) {
      throw err
    }
    throw new PnpmError('TARBALL_INTEGRITY', `Got unexpected checksum for "${url}". Wanted "${err.expected as string}". Got "${err.found as string}".`)
  }

  // Write the verified data to file
  await fsPromises.writeFile(tmpPath, data)
}

/**
 * Extracts a zip file to the target directory.
 *
 * @param zipPath - Path to the zip file
 * @param basename - Base name of the file (without extension)
 * @param targetDir - Directory where contents should be extracted
 * @param ignoreEntry - Optional regex matched against the entry path relative to `basename`;
 *   matching entries are skipped.
 * @throws {PnpmError} When extraction fails or path traversal is detected
 */
async function extractZipToTarget (
  zipPath: string,
  basename: string,
  targetDir: string,
  ignoreEntry?: RegExp
): Promise<void> {
  const zip = new AdmZip(zipPath)
  const nodeDir = basename === '' ? targetDir : path.dirname(targetDir)

  // Validate basename/prefix doesn't escape the target directory
  if (basename !== '') {
    validatePathSecurity(nodeDir, basename)
  }

  const basenamePrefix = basename === '' ? '' : `${basename}/`
  // Normalize `ignoreEntry` to a stateless regex. `.test()` on a `/g` or `/y` regex
  // advances `lastIndex` between calls, which would cause inconsistent skips across
  // entries in this loop.
  const testEntry = toStatelessTester(ignoreEntry)

  // Extract each entry with path validation to prevent path traversal attacks
  for (const entry of zip.getEntries()) {
    const entryPath = entry.entryName
    validatePathSecurity(nodeDir, entryPath)
    if (testEntry) {
      const relative = basenamePrefix && entryPath.startsWith(basenamePrefix)
        ? entryPath.slice(basenamePrefix.length)
        : entryPath
      if (testEntry(relative)) continue
    }
    zip.extractEntryTo(entry, nodeDir, true, true)
  }

  const extractedDir = path.join(nodeDir, basename)
  await renameOverwrite(extractedDir, targetDir)
}

function toStatelessTester (regex: RegExp | undefined): ((input: string) => boolean) | undefined {
  if (!regex) return undefined
  // `/g` and `/y` make `RegExp.prototype.test` stateful via `lastIndex`.
  // Strip those flags by cloning into a fresh RegExp with only the safe flags.
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
 * @param basePath - The base directory that should contain the target
 * @param targetPath - The relative path to validate
 * @throws {PnpmError} When path traversal is detected
 */
function validatePathSecurity (basePath: string, targetPath: string): void {
  // Explicitly reject absolute paths - they should never be allowed as prefixes or entry names
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
