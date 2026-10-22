import { promises as fs, type Stats } from 'node:fs'
import path from 'node:path'

import { pkgRequiresBuild } from '@pnpm/building.pkg-requires-build'
import { isError, PnpmError } from '@pnpm/error'
import type {
  DirectoryFetcher,
  DirectoryFetcherOptions,
  LocalDirPackageImportMethod,
} from '@pnpm/fetching.fetcher-base'
import { packlist } from '@pnpm/fs.packlist'
import { logger } from '@pnpm/logger'
import type { FilesMap } from '@pnpm/store.cafs-types'
import type { DependencyManifest } from '@pnpm/types'
import { safeReadParentPublishManifest, safeReadProjectManifestOnly } from '@pnpm/workspace.project-manifest-reader'

const directoryFetcherLogger = logger('directory-fetcher')

export interface CreateDirectoryFetcherOptions {
  includeOnlyPackageFiles?: boolean
  localDirPackageImportMethod?: LocalDirPackageImportMethod
  resolveSymlinks?: boolean
}

/** Options only a caller that lists a directory, rather than fetching it, sets. */
export interface ListFilesOptions {
  /** List a symlink by its own path, a directory link included, without walking into it. Ignored under `resolveSymlinks`. */
  preserveSymlinks?: boolean
}

export function createDirectoryFetcher (
  fetcherOpts?: CreateDirectoryFetcherOptions
): { directory: DirectoryFetcher } {
  const packageImportMethod = fetcherOpts?.localDirPackageImportMethod ?? 'hardlink'
  const directoryFetcher: DirectoryFetcher = async (cafs, resolution, opts) => {
    // Use path.resolve so absolute directories (e.g. cross-drive Windows paths
    // stored by `file:` deps) are respected instead of being concatenated
    // onto lockfileDir.
    const dir = path.resolve(opts.lockfileDir, resolution.directory)
    // An injected dependency whose packed content is the output of its own
    // lifecycle scripts (a project with `publishConfig.directory` built by
    // `prepare`) has no source directory on a fresh install: the scripts run
    // after linking, and the built output is imported afterwards. Inject an
    // empty copy so the install can proceed instead of failing on the
    // not-yet-built directory. Any other missing directory still fails.
    if (!await dirExists(dir)) {
      const manifest = await safeReadParentPublishManifest(dir) as DependencyManifest | null
      if (manifest != null) {
        return {
          local: true,
          filesMap: new Map(),
          packageImportMethod,
          manifest,
          requiresBuild: false,
          sourceExists: false,
        }
      }
    }
    return {
      ...await fetchFromDir(dir, fetcherOpts ?? {}),
      sourceExists: true,
    }
  }

  return {
    directory: directoryFetcher,
  }
}

export type FetchFromDirOptions = Omit<DirectoryFetcherOptions, 'lockfileDir'> & CreateDirectoryFetcherOptions & {
  /** Confine the walk, and the links it follows, to this directory, which holds the one walked. */
  packageRoot?: string
}

async function dirExists (dir: string): Promise<boolean> {
  try {
    await fs.stat(dir)
    return true
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') return false
    throw err
  }
}

export interface FetchResult {
  local: true
  filesMap: FilesMap
  filesStats?: Record<string, Stats | null>
  packageImportMethod: LocalDirPackageImportMethod
  manifest: DependencyManifest
  requiresBuild: boolean
}

/**
 * The files `fetchFromDir` lists, without reading the directory's manifest
 * unless packlist mode needs it. For a caller that only compares listings and
 * must not fail on a `package.json` another process is still writing.
 */
export async function listFilesFromDir (
  dir: string,
  opts: FetchFromDirOptions & ListFilesOptions
): Promise<Pick<FetchResult, 'filesMap' | 'filesStats'>> {
  const start = opts.packageRoot == null ? dir : await resolveInsidePackage(dir, opts.packageRoot)
  if (opts.includeOnlyPackageFiles) {
    const files = await packlist(start)
    return { filesMap: new Map<string, string>(files.map((file) => [file, path.join(start, file)])) }
  }
  return _fetchAllFilesFromDir(await createReadFileStat(start, opts), start, '', new Set([await dirId(start, await fs.stat(start))]))
}

/** The real path of `dir`, which a walk uses so a link pointed elsewhere after this check is not followed. */
async function resolveInsidePackage (dir: string, packageRoot: string): Promise<string> {
  const start = await fs.realpath(dir)
  if (isOutside(await fs.realpath(packageRoot), start)) {
    throw new PnpmError('INVALID_PATH', `Path "${dir}" is outside the package directory "${packageRoot}"`)
  }
  return start
}

function isOutside (root: string, realPath: string): boolean {
  const relative = path.relative(root, realPath)
  return relative === '..' || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)
}

export async function fetchFromDir (dir: string, opts: FetchFromDirOptions): Promise<FetchResult> {
  const packageImportMethod = opts.localDirPackageImportMethod ?? 'hardlink'
  const { filesMap, filesStats } = await listFilesFromDir(dir, opts)
  // In a regular pnpm workspace a dependency is not expected to lack a package.json file.
  // Safe read was added to support the Bit workspace in which the components have no package.json files.
  // Related PR in Bit: https://github.com/teambit/bit/pull/5251
  const manifest = await safeReadProjectManifestOnly(dir) as DependencyManifest ?? undefined
  const requiresBuild = pkgRequiresBuild(manifest, filesMap)
  return {
    local: true,
    filesMap,
    filesStats,
    packageImportMethod,
    manifest,
    requiresBuild,
  }
}

async function createReadFileStat (
  dir: string,
  opts: Pick<FetchFromDirOptions, 'includeOnlyPackageFiles' | 'packageRoot' | 'resolveSymlinks'> & ListFilesOptions
): Promise<ReadFileStat> {
  const readStat = opts.resolveSymlinks === true ? realFileStat : opts.preserveSymlinks === true ? linkStat : fileStat
  if (opts.includeOnlyPackageFiles !== false && opts.packageRoot == null) return readStat
  const rootDir = opts.packageRoot ?? dir
  const root = await fs.realpath(rootDir)
  return async (filePath) => {
    const result = await readStat(filePath)
    if (result == null) return null
    if (!(await fs.lstat(filePath)).isSymbolicLink()) return result
    if (isOutside(root, await fs.realpath(filePath))) {
      throw new PnpmError('INVALID_PATH', `Path "${filePath}" is outside the package directory "${rootDir}"`)
    }
    return result
  }
}

/**
 * `ancestors` holds the identity of `dir` and of each directory above it in
 * the walk. A link back to one of them is not walked into, since the walk would
 * never end.
 */
async function _fetchAllFilesFromDir (
  readFileStat: ReadFileStat,
  dir: string,
  relativeDir: string,
  ancestors: ReadonlySet<string>
): Promise<Pick<FetchResult, 'filesMap' | 'filesStats'>> {
  const filesMap: FilesMap = new Map()
  const filesStats: Record<string, Stats | null> = {}
  const files = await fs.readdir(dir)
  await Promise.all(files
    .filter((file) => file !== 'node_modules')
    .map((file) => handleDirEntry(readFileStat, { dir, file, relativeDir, ancestors }, filesMap, filesStats))
  )
  return { filesMap, filesStats }
}

interface DirEntry {
  dir: string
  file: string
  relativeDir: string
  ancestors: ReadonlySet<string>
}

async function handleDirEntry (
  readFileStat: ReadFileStat,
  { dir, file, relativeDir, ancestors }: DirEntry,
  filesMap: FilesMap,
  filesStats: Record<string, Stats | null>
): Promise<void> {
  const fileStatResult = await readFileStat(path.join(dir, file))
  if (!fileStatResult) return
  const { filePath, stat } = fileStatResult
  const relativeSubdir = `${relativeDir}${relativeDir ? '/' : ''}${file}`
  if (stat.isDirectory()) {
    const id = await dirId(filePath, stat)
    if (ancestors.has(id)) {
      directoryFetcherLogger.debug({ linkToAncestor: filePath })
      return
    }
    const subFetchResult = await _fetchAllFilesFromDir(readFileStat, filePath, relativeSubdir, new Set([...ancestors, id]))
    for (const [key, value] of subFetchResult.filesMap) {
      filesMap.set(key, value)
    }
    Object.assign(filesStats, subFetchResult.filesStats)
  } else {
    filesMap.set(relativeSubdir, filePath)
    filesStats[relativeSubdir] = fileStatResult.stat
  }
}

/**
 * A directory's identity: its device and inode, or its real path where the
 * filesystem reports no inode, as some Windows drives do.
 */
async function dirId (dir: string, stat: Pick<Stats, 'dev' | 'ino'>): Promise<string> {
  return stat.ino === 0 ? `path:${await fs.realpath(dir)}` : `${stat.dev}:${stat.ino}`
}

interface FileStatResult {
  filePath: string
  stat: Stats
}

type ReadFileStat = (filePath: string) => Promise<FileStatResult | null>

async function realFileStat (filePath: string): Promise<FileStatResult | null> {
  let stat = await fs.lstat(filePath)
  if (!stat.isSymbolicLink()) {
    return { filePath, stat }
  }
  try {
    filePath = await fs.realpath(filePath)
    stat = await fs.stat(filePath)
    return { filePath, stat }
  } catch (err: unknown) {
    // Broken symlinks are skipped
    if (isError(err) && 'code' in err && err.code === 'ENOENT') {
      directoryFetcherLogger.debug({ brokenSymlink: filePath })
      return null
    }
    throw err
  }
}

/** A path as it is, a link not followed. */
async function linkStat (filePath: string): Promise<FileStatResult> {
  return { filePath, stat: await fs.lstat(filePath) }
}

async function fileStat (filePath: string): Promise<FileStatResult | null> {
  try {
    return {
      filePath,
      stat: await fs.stat(filePath),
    }
  } catch (err: unknown) {
    // Broken symlinks are skipped
    if (isError(err) && 'code' in err && err.code === 'ENOENT') {
      directoryFetcherLogger.debug({ brokenSymlink: filePath })
      return null
    }
    throw err
  }
}

