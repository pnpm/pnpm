import path from 'path'
import { Cafs, DeferredManifestPromise } from '@pnpm/fetcher-base'
import { DirectoryResolution } from '@pnpm/resolver-base'
import fromPairs from 'ramda/src/fromPairs'
import loadJsonFile from 'load-json-file'
import packlist from 'npm-packlist'

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
