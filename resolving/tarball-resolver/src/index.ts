import type { PkgResolutionId, ResolveResult, TarballResolution } from '@pnpm/resolver-base'
import type { FetchFromRegistry } from '@pnpm/fetching-types'

export interface TarballResolveResult extends ResolveResult {
  normalizedBareSpecifier: string
  resolution: TarballResolution
  resolvedVia: 'url'
}

export async function resolveFromTarball (
  fetchFromRegistry: FetchFromRegistry,
  wantedDependency: { bareSpecifier: string }
): Promise<TarballResolveResult | null> {
  if (!wantedDependency.bareSpecifier.startsWith('http:') && !wantedDependency.bareSpecifier.startsWith('https:')) {
    return null
  }

  // The URL is normalized to remove the port if it is the default port of the protocol.
  const normalizedBareSpecifier = new URL(wantedDependency.bareSpecifier).toString()
  let resolvedUrl: string

  // If there are redirects and the response is immutable, we want to get the final URL address
  const response = await fetchFromRegistry(normalizedBareSpecifier, { method: 'HEAD' })
  if (response?.headers?.get('cache-control')?.includes('immutable')) {
    resolvedUrl = response.url
  } else {
    resolvedUrl = normalizedBareSpecifier
  }

  return {
    id: normalizedBareSpecifier as PkgResolutionId,
    normalizedBareSpecifier,
    resolution: {
      tarball: resolvedUrl,
    },
    resolvedVia: 'url',
  }
}
