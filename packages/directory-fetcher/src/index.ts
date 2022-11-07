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
}

export function createDirectoryFetcher (
  opts?: CreateDirectoryFetcherOptions
) {
  const fetchFromDir = opts?.includeOnlyPackageFiles ? fetchPackageFilesFromDir : fetchAllFilesFromDir

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
  return fetchAllFilesFromDir(dir, opts)
}

async function fetchAllFilesFromDir (
  dir: string,
  opts: FetchFromDirOpts
) {
  const filesIndex = await _fetchAllFilesFromDir(dir)
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
  dir: string,
  relativeDir = ''
): Promise<Record<string, string>> {
  const filesIndex: Record<string, string> = {}
  const files = await fs.readdir(dir)
  await Promise.all(files
    .filter((file) => file !== 'node_modules')
    .map(async (file) => {
      const filePath = path.join(dir, file)
      let stat: Stats
      try {
        stat = await fs.stat(filePath)
      } catch (err: any) { // eslint-disable-line @typescript-eslint/no-explicit-any
        // Broken symlinks are skipped
        if (err.code === 'ENOENT') {
          directoryFetcherLogger.debug({ brokenSymlink: filePath })
          return
        }
        throw err
      }
      const relativeSubdir = `${relativeDir}${relativeDir ? '/' : ''}${file}`
      if (stat.isDirectory()) {
        const subFilesIndex = await _fetchAllFilesFromDir(filePath, relativeSubdir)
        Object.assign(filesIndex, subFilesIndex)
      } else {
        filesIndex[relativeSubdir] = filePath
      }
    })
  )
  return filesIndex
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
