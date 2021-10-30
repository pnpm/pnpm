import { promises as fs } from 'fs'
import path from 'path'
import { Cafs, DeferredManifestPromise } from '@pnpm/fetcher-base'
import { DirectoryResolution } from '@pnpm/resolver-base'
import loadJsonFile from 'load-json-file'

export interface DirectoryFetcherOptions {
  manifest?: DeferredManifestPromise
}

export default () => {
  return {
    directory: (
      cafs: Cafs,
      resolution: DirectoryResolution,
      opts: DirectoryFetcherOptions
    ) => fetchFromDir(resolution.directory, opts),
  }
}

export async function fetchFromDir (
  dir: string,
  opts: DirectoryFetcherOptions
) {
  const filesIndex: Record<string, string> = {}
  await mapDirectory(dir, dir, filesIndex)
  if (opts.manifest) {
    opts.manifest.resolve(await loadJsonFile(path.join(dir, 'package.json')))
  }
  return {
    local: true,
    filesIndex,
    packageImportMethod: 'hardlink',
  }
}

async function mapDirectory (
  rootDir: string,
  currDir: string,
  index: Record<string, string>
) {
  const files = await fs.readdir(currDir)
  await Promise.all(files.filter((file) => file !== 'node_modules').map(async (file) => {
    const fullPath = path.join(currDir, file)
    const stat = await fs.stat(fullPath)
    if (stat.isDirectory()) {
      await mapDirectory(rootDir, fullPath, index)
      return
    }
    if (stat.isFile()) {
      const relativePath = path.relative(rootDir, fullPath)
      index[relativePath] = fullPath
    }
  }))
}
