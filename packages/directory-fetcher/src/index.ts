import path from 'path'
import { Cafs, DeferredManifestPromise } from '@pnpm/fetcher-base'
import { DirectoryResolution } from '@pnpm/resolver-base'
import fromPairs from 'ramda/src/fromPairs'
import loadJsonFile from 'load-json-file'
import packlist from 'npm-packlist'

export interface DirectoryFetcherOptions {
  lockfileDir: string
  manifest?: DeferredManifestPromise
}

export default () => {
  return {
    directory: (
      cafs: Cafs,
      resolution: DirectoryResolution,
      opts: DirectoryFetcherOptions
    ) => {
      const dir = path.join(opts.lockfileDir, resolution.directory)
      return fetchFromDir(dir, opts)
    },
  }
}

export async function fetchFromDir (
  dir: string,
  opts: Omit<DirectoryFetcherOptions, 'lockfileDir'>
) {
  const files = await packlist({ path: dir })
  const filesIndex: Record<string, string> = fromPairs(files.map((file) => [file, path.join(dir, file)]))
  if (opts.manifest) {
    opts.manifest.resolve(await loadJsonFile(path.join(dir, 'package.json')))
  }
  return {
    local: true as const,
    filesIndex,
    packageImportMethod: 'hardlink' as const,
  }
}
