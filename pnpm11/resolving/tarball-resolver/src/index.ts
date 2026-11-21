import type { FetchFromRegistry, GetAuthHeader, RetryTimeoutOptions } from '@pnpm/fetching.types'
import { isUrlSecureForCredentials } from '@pnpm/network.fetch'
import type { LatestInfo, LatestQuery, PkgResolutionId, ResolveResult, TarballResolution } from '@pnpm/resolving.resolver-base'

export interface TarballResolveResult extends ResolveResult {
  normalizedBareSpecifier: string
  resolution: TarballResolution
  resolvedVia: 'url'
}

export async function resolveFromTarball (
  fetchFromRegistry: FetchFromRegistry,
  wantedDependency: { alias?: string, bareSpecifier: string },
  opts?: { getAuthHeader?: GetAuthHeader, retry?: RetryTimeoutOptions }
): Promise<TarballResolveResult | null> {
  if (!wantedDependency.bareSpecifier.startsWith('http:') && !wantedDependency.bareSpecifier.startsWith('https:')) {
    return null
  }

  // The URL is normalized to remove the port if it is the default port of the protocol.
  const normalizedBareSpecifier = new URL(wantedDependency.bareSpecifier).toString()
  let resolvedUrl: string

  // If there are redirects and the response is immutable, we want to get the final URL address
  const getAuthHeader = (url: string): string | undefined => opts?.getAuthHeader?.(url, { pkgName: wantedDependency.alias })
  const requestOptions = {
    authHeaderValue: isUrlSecureForCredentials(normalizedBareSpecifier) ? getAuthHeader(normalizedBareSpecifier) : undefined,
    getRedirectAuthHeader: getAuthHeader,
    retry: opts?.retry,
  }
  let response = await fetchFromRegistry(normalizedBareSpecifier, { ...requestOptions, method: 'HEAD' })
  if (response.status === 405) {
    response = await fetchFromRegistry(normalizedBareSpecifier, { ...requestOptions, method: 'GET' })
  }
  if (response.status >= 200 && response.status < 300 && response.headers?.get('cache-control')?.split(',').some(directive => directive.trim().toLowerCase() === 'immutable')) {
    resolvedUrl = response.url
  } else {
    resolvedUrl = normalizedBareSpecifier
  }
  // The resolution is already decided, so a body that failed after its headers must not fail it.
  await response.body?.cancel().catch(() => {})

  return {
    id: normalizedBareSpecifier as PkgResolutionId,
    normalizedBareSpecifier,
    resolution: {
      tarball: resolvedUrl,
    },
    resolvedVia: 'url',
  }
}

// URL tarballs lock to the exact URL and have no concept of "latest". Claim
// the dep so the dispatcher stops; the caller still surfaces a ref-mismatch
// report if the lockfile points at a different URL than before.
export async function resolveLatestFromTarball (query: LatestQuery): Promise<LatestInfo | undefined> {
  const bareSpecifier = query.wantedDependency.bareSpecifier
  if (!bareSpecifier?.startsWith('http:') && !bareSpecifier?.startsWith('https:')) return undefined
  return {}
}
