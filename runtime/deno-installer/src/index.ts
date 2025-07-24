import { getDenoBinLocationForCurrentOS } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import { type FetchFromRegistry } from '@pnpm/fetching-types'
import { type DenoRuntimeFetcher, type FetchResult } from '@pnpm/fetcher-base'
import { type WantedDependency, type DenoRuntimeResolution, type ResolveResult } from '@pnpm/resolver-base'
import semver from 'semver'
import { type PkgResolutionId } from '@pnpm/types'
import { type NpmResolver } from '@pnpm/npm-resolver'

export interface DenoRuntimeResolveResult extends ResolveResult {
  resolution: DenoRuntimeResolution
  resolvedVia: 'github.com/denoland/deno'
}

export async function resolveDenoRuntime (
  ctx: {
    fetchFromRegistry: FetchFromRegistry
    rawConfig: Record<string, string>
    offline?: boolean
    resolveFromNpm: NpmResolver
  },
  wantedDependency: WantedDependency
): Promise<DenoRuntimeResolveResult | null> {
  if (wantedDependency.alias !== 'deno' || !wantedDependency.bareSpecifier?.startsWith('runtime:')) return null
  const versionSpec = wantedDependency.bareSpecifier.substring('runtime:'.length)
  // We use the npm registry for version resolution as it is easier than using the GitHub API for releases,
  // which uses pagination (e.g. https://api.github.com/repos/denoland/deno/releases?per_page=100).
  const npmResolution = await ctx.resolveFromNpm({ ...wantedDependency, bareSpecifier: versionSpec }, {})
  const version = npmResolution.manifest.version

  return {
    id: `deno@runtime:${version}` as PkgResolutionId,
    normalizedBareSpecifier: `runtime:${versionSpec}`,
    resolvedVia: 'github.com/denoland/deno',
    manifest: {
      name: 'deno',
      version,
      bin: getDenoBinLocationForCurrentOS(),
    },
    resolution: {
      type: 'denoRuntime',
      integrity: '',
    },
  }
}

export function createDenoRuntimeFetcher (ctx: {
  fetch: FetchFromRegistry
  rawConfig: Record<string, string>
  offline?: boolean
}): { denoRuntime: DenoRuntimeFetcher } {
  const fetchDenoRuntime: DenoRuntimeFetcher = async (cafs, resolution, opts) => {
    if (!opts.pkg.version) {
      throw new PnpmError('CANNOT_FETCH_DENO_WITHOUT_VERSION', 'Cannot fetch Deno without a version')
    }
    if (ctx.offline) {
      throw new PnpmError('CANNOT_DOWNLOAD_DENO_OFFLINE', 'Cannot download Deno because offline mode is enabled.')
    }
    const version = opts.pkg.version
    console.log(version)

    throw new Error('not implemented')
  }
  return {
    denoRuntime: fetchDenoRuntime,
  }
}
