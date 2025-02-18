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
  shallowLStats?: Record<string, Stats>
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
  const { filesIndex, shallowLStats } = await _fetchAllFilesFromDir(readFileStat, dir)
  // In a regular pnpm workspace it will probably never happen that a dependency has no package.json file.
  // Safe read was added to support the Bit workspace in which the components have no package.json files.
  // Related PR in Bit: https://github.com/teambit/bit/pull/5251
  const manifest = await safeReadProjectManifestOnly(dir) as DependencyManifest ?? undefined
  const requiresBuild = pkgRequiresBuild(manifest, filesIndex)
  return {
    local: true,
    filesIndex,
    shallowLStats,
    packageImportMethod: 'hardlink',
    manifest,
    requiresBuild,
  }
}

interface SubFetchResult {
  filesIndex: Record<string, string>
  shallowLStats: Record<string, Stats>
}

async function _fetchAllFilesFromDir (
  readFileStat: ReadFileStat,
  dir: string,
  relativeDir = ''
): Promise<SubFetchResult> {
  const filesIndex: Record<string, string> = {}
  const shallowLStats: Record<string, Stats> = {}
  const files = await fs.readdir(dir)
  await Promise.all(files
    .filter((file) => file !== 'node_modules')
    .map(async (file) => {
      const fileStatResult = await readFileStat(path.join(dir, file))
      if (!fileStatResult) return
      const { filePath, stat, shallowLStat } = fileStatResult
      const relativeSubdir = `${relativeDir}${relativeDir ? '/' : ''}${file}`
      if (stat.isDirectory()) {
        const subResult = await _fetchAllFilesFromDir(readFileStat, filePath, relativeSubdir)
        Object.assign(filesIndex, subResult.filesIndex)
        Object.assign(shallowLStats, subResult.shallowLStats)
      } else {
        filesIndex[relativeSubdir] = filePath
        shallowLStats[relativeSubdir] = shallowLStat!
      }
    })
  )
  return { filesIndex, shallowLStats }
}

interface FileStatResult {
  filePath: string
  stat: Stats
  shallowLStat?: Stats
}

type ReadFileStat = (filePath: string) => Promise<FileStatResult | null>

interface RealFileStatResult extends FileStatResult {
  shallowLStat: Stats
}

async function realFileStat (filePath: string): Promise<RealFileStatResult | null> {
  let stat = await fs.lstat(filePath)
  const shallowLStat = stat
  if (!stat.isSymbolicLink()) {
    return { filePath, stat, shallowLStat }
  }
  try {
    filePath = await fs.realpath(filePath)
    stat = await fs.stat(filePath)
    return { filePath, stat, shallowLStat }
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
