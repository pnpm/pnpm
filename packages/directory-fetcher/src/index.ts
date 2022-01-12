import path from 'path'
import { Cafs, DeferredManifestPromise } from '@pnpm/fetcher-base'
import { safeReadProjectManifestOnly } from '@pnpm/read-project-manifest'
import { DirectoryResolution } from '@pnpm/resolver-base'
import fromPairs from 'ramda/src/fromPairs'
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
