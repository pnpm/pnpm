import { type PkgResolutionId, type ResolveResult, type TarballResolution } from '@pnpm/resolver-base'
import { type FetchFromRegistry } from '@pnpm/fetching-types'

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

  if (isRepository(wantedDependency.bareSpecifier)) return null

  // The URL is normalized to remove the port if it is the default port of the protocol.
  let normalizedBareSpecifier = new URL(wantedDependency.bareSpecifier).toString()
  if (normalizedBareSpecifier.endsWith('/') && !wantedDependency.bareSpecifier.endsWith('/')) {
    normalizedBareSpecifier = normalizedBareSpecifier.slice(0, -1)
  }
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

const GIT_HOSTERS = new Set([
  'github.com',
  'gitlab.com',
  'bitbucket.org',
])

function isRepository (bareSpecifier: string): boolean {
  const url = new URL(bareSpecifier)
  if (url.hash && url.hash.includes('/')) {
    url.hash = encodeURIComponent(url.hash.substring(1))
    bareSpecifier = url.href
  }
  if (bareSpecifier.endsWith('/')) {
    bareSpecifier = bareSpecifier.slice(0, -1)
  }
  const parts = bareSpecifier.split('/')
  return (parts.length === 5 && GIT_HOSTERS.has(parts[2]))
}
