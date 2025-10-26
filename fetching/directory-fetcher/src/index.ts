import { promises as fs, type Stats } from 'fs'
import path from 'path'
import util from 'util'
import { pkgRequiresBuild } from '@pnpm/exec.pkg-requires-build'
import type { DirectoryFetcher, DirectoryFetcherOptions } from '@pnpm/fetcher-base'
import { logger } from '@pnpm/logger'
import { packlist } from '@pnpm/fs.packlist'
import { safeReadProjectManifestOnly } from '@pnpm/read-project-manifest'
import { type DependencyManifest } from '@pnpm/types'

const directoryFetcherLogger = logger('directory-fetcher')

export interface CreateDirectoryFetcherOptions {
  includeOnlyPackageFiles?: boolean
  resolveSymlinks?: boolean
}

export function createDirectoryFetcher (
  opts?: CreateDirectoryFetcherOptions
): { directory: DirectoryFetcher } {
  const readFileStat: ReadFileStat = opts?.resolveSymlinks === true ? realFileStat : fileStat
  const fetchFromDir = opts?.includeOnlyPackageFiles ? fetchPackageFilesFromDir : fetchAllFilesFromDir.bind(null, readFileStat)

  const directoryFetcher: DirectoryFetcher = (cafs, resolution, opts) => {
    const dir = path.join(opts.lockfileDir, resolution.directory)
    return fetchFromDir(dir)
  }

  return {
    directory: directoryFetcher,
  }
}

export type FetchFromDirOptions = Omit<DirectoryFetcherOptions, 'lockfileDir'> & CreateDirectoryFetcherOptions

export interface FetchResult {
  local: true
  filesIndex: Record<string, string>
  filesStats?: Record<string, Stats | null>
  packageImportMethod: 'hardlink'
  manifest: DependencyManifest
  requiresBuild: boolean
}

export async function fetchFromDir (dir: string, opts: FetchFromDirOptions): Promise<FetchResult> {
  if (opts.includeOnlyPackageFiles) {
    return fetchPackageFilesFromDir(dir)
  }
  const readFileStat: ReadFileStat = opts?.resolveSymlinks === true ? realFileStat : fileStat
  return fetchAllFilesFromDir(readFileStat, dir)
}

async function fetchAllFilesFromDir (
  readFileStat: ReadFileStat,
  dir: string
): Promise<FetchResult> {
  const { filesIndex, filesStats } = await _fetchAllFilesFromDir(readFileStat, dir)
  // In a regular pnpm workspace it will probably never happen that a dependency has no package.json file.
  // Safe read was added to support the Bit workspace in which the components have no package.json files.
  // Related PR in Bit: https://github.com/teambit/bit/pull/5251
  const manifest = await safeReadProjectManifestOnly(dir) as DependencyManifest ?? undefined
  const requiresBuild = pkgRequiresBuild(manifest, filesIndex)
  return {
    local: true,
    filesIndex,
    filesStats,
    packageImportMethod: 'hardlink',
    manifest,
    requiresBuild,
  }
}

async function _fetchAllFilesFromDir (
  readFileStat: ReadFileStat,
  dir: string,
  relativeDir = ''
): Promise<Pick<FetchResult, 'filesIndex' | 'filesStats'>> {
  const filesIndex: Record<string, string> = {}
  const filesStats: Record<string, Stats | null> = {}
  const files = await fs.readdir(dir)
  await Promise.all(files
    .filter((file) => file !== 'node_modules')
    .map(async (file) => {
      const fileStatResult = await readFileStat(path.join(dir, file))
      if (!fileStatResult) return
      const { filePath, stat } = fileStatResult
      const relativeSubdir = `${relativeDir}${relativeDir ? '/' : ''}${file}`
      if (stat.isDirectory()) {
        const subFetchResult = await _fetchAllFilesFromDir(readFileStat, filePath, relativeSubdir)
        Object.assign(filesIndex, subFetchResult.filesIndex)
        Object.assign(filesStats, subFetchResult.filesStats)
      } else {
        filesIndex[relativeSubdir] = filePath
        filesStats[relativeSubdir] = fileStatResult.stat
      }
    })
  )
  return { filesIndex, filesStats }
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
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      directoryFetcherLogger.debug({ brokenSymlink: filePath })
      return null
    }
    throw err
  }
}

async function fileStat (filePath: string): Promise<FileStatResult | null> {
  try {
    return {
      filePath,
      stat: await fs.stat(filePath),
    }
  } catch (err: unknown) {
    // Broken symlinks are skipped
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      directoryFetcherLogger.debug({ brokenSymlink: filePath })
      return null
    }
    throw err
  }
}

async function fetchPackageFilesFromDir (dir: string): Promise<FetchResult> {
  const files = await packlist(dir)
  const filesIndex: Record<string, string> = Object.fromEntries(files.map((file) => [file, path.join(dir, file)]))
  // In a regular pnpm workspace it will probably never happen that a dependency has no package.json file.
  // Safe read was added to support the Bit workspace in which the components have no package.json files.
  // Related PR in Bit: https://github.com/teambit/bit/pull/5251
  const manifest = await safeReadProjectManifestOnly(dir) as DependencyManifest ?? undefined
  const requiresBuild = pkgRequiresBuild(manifest, filesIndex)
  return {
    local: true,
    filesIndex,
    packageImportMethod: 'hardlink',
    manifest,
    requiresBuild,
  }
}
