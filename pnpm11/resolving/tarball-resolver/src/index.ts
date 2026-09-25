import type { FetchFromRegistry } from '@pnpm/fetching.types'
import type { LatestInfo, LatestQuery, PkgResolutionId, ResolveResult, TarballResolution } from '@pnpm/resolving.resolver-base'

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

// URL tarballs lock to the exact URL — no concept of "latest". Claim the dep
// so the dispatcher stops; the caller still surfaces a ref-mismatch report
// if the lockfile points at a different URL than before.
export async function resolveLatestFromTarball (query: LatestQuery): Promise<LatestInfo | undefined> {
  const bareSpecifier = query.wantedDependency.bareSpecifier
  if (!bareSpecifier?.startsWith('http:') && !bareSpecifier?.startsWith('https:')) return undefined
  return {}
}
