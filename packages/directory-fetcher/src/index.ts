import { promises as fs, Stats } from 'fs'
import path from 'path'
import type { DirectoryFetcher, DirectoryFetcherOptions } from '@pnpm/fetcher-base'
import { logger } from '@pnpm/logger'
import { safeReadProjectManifestOnly } from '@pnpm/read-project-manifest'
import fromPairs from 'ramda/src/fromPairs'
import packlist from 'npm-packlist'

const directoryFetcherLogger = logger('directory-fetcher')

export interface CreateDirectoryFetcherOptions {
  includeOnlyPackageFiles?: boolean
  resolveSymlinks?: boolean
}

export function createDirectoryFetcher (
  opts?: CreateDirectoryFetcherOptions
) {
  const readFileStat: ReadFileStat = opts?.resolveSymlinks === true ? realFileStat : fileStat
  const fetchFromDir = opts?.includeOnlyPackageFiles ? fetchPackageFilesFromDir : fetchAllFilesFromDir.bind(null, readFileStat)

  const directoryFetcher: DirectoryFetcher = (cafs, resolution, opts) => {
    const dir = path.join(opts.lockfileDir, resolution.directory)
    return fetchFromDir(dir, opts)
  }

  return {
    directory: directoryFetcher,
  }
}

type FetchFromDirOpts = Omit<DirectoryFetcherOptions, 'lockfileDir'>

export async function fetchFromDir (
  dir: string,
  opts: FetchFromDirOpts & CreateDirectoryFetcherOptions
) {
  if (opts.includeOnlyPackageFiles) {
    return fetchPackageFilesFromDir(dir, opts)
  }
  const readFileStat: ReadFileStat = opts?.resolveSymlinks === true ? realFileStat : fileStat
  return fetchAllFilesFromDir(readFileStat, dir, opts)
}

async function fetchAllFilesFromDir (
  readFileStat: ReadFileStat,
  dir: string,
  opts: FetchFromDirOpts
) {
  const filesIndex = await _fetchAllFilesFromDir(readFileStat, dir)
  if (opts.manifest) {
    // In a regular pnpm workspace it will probably never happen that a dependency has no package.json file.
    // Safe read was added to support the Bit workspace in which the components have no package.json files.
    // Related PR in Bit: https://github.com/teambit/bit/pull/5251
    const manifest = await safeReadProjectManifestOnly(dir) ?? {}
    opts.manifest.resolve(manifest as any) // eslint-disable-line @typescript-eslint/no-explicit-any
  }
  return {
    local: true as const,
    filesIndex,
    packageImportMethod: 'hardlink' as const,
  }
}

async function _fetchAllFilesFromDir (
  readFileStat: ReadFileStat,
  dir: string,
  relativeDir = ''
): Promise<Record<string, string>> {
  const filesIndex: Record<string, string> = {}
  const files = await fs.readdir(dir)
  await Promise.all(files
    .filter((file) => file !== 'node_modules')
    .map(async (file) => {
      const { filePath, stat } = await readFileStat(path.join(dir, file))
      if (!filePath) return
      const relativeSubdir = `${relativeDir}${relativeDir ? '/' : ''}${file}`
      if (stat.isDirectory()) {
        const subFilesIndex = await _fetchAllFilesFromDir(readFileStat, filePath, relativeSubdir)
        Object.assign(filesIndex, subFilesIndex)
      } else {
        filesIndex[relativeSubdir] = filePath
      }
    })
  )
  return filesIndex
}

type ReadFileStat = (filePath: string) => Promise<{ filePath: string, stat: Stats } | { filePath: null, stat: null }>

async function realFileStat (filePath: string): Promise<{ filePath: string, stat: Stats } | { filePath: null, stat: null }> {
  let stat = await fs.lstat(filePath)
  if (!stat.isSymbolicLink()) {
    return { filePath, stat }
  }
  try {
    filePath = await fs.realpath(filePath)
    stat = await fs.stat(filePath)
    return { filePath, stat }
  } catch (err: any) { // eslint-disable-line @typescript-eslint/no-explicit-any
    // Broken symlinks are skipped
    if (err.code === 'ENOENT') {
      directoryFetcherLogger.debug({ brokenSymlink: filePath })
      return { filePath: null, stat: null }
    }
    throw err
  }
}

async function fileStat (filePath: string): Promise<{ filePath: string, stat: Stats } | { filePath: null, stat: null }> {
  try {
    return {
      filePath,
      stat: await fs.stat(filePath),
    }
  } catch (err: any) { // eslint-disable-line @typescript-eslint/no-explicit-any
    // Broken symlinks are skipped
    if (err.code === 'ENOENT') {
      directoryFetcherLogger.debug({ brokenSymlink: filePath })
      return { filePath: null, stat: null }
    }
    throw err
  }
}

async function fetchPackageFilesFromDir (
  dir: string,
  opts: FetchFromDirOpts
) {
  const files = await packlist({ path: dir })
  const filesIndex: Record<string, string> = fromPairs(files.map((file) => [file, path.join(dir, file)]))
  if (opts.manifest) {
    // In a regular pnpm workspace it will probably never happen that a dependency has no package.json file.
    // Safe read was added to support the Bit workspace in which the components have no package.json files.
    // Related PR in Bit: https://github.com/teambit/bit/pull/5251
    const manifest = await safeReadProjectManifestOnly(dir) ?? {}
    opts.manifest.resolve(manifest as any) // eslint-disable-line @typescript-eslint/no-explicit-any
  }
  return {
    local: true as const,
    filesIndex,
    packageImportMethod: 'hardlink' as const,
  }
}
