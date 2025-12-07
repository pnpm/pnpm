import { type PkgResolutionId, type ResolveResult, type TarballResolution } from '@pnpm/resolver-base'
import { type FetchFromRegistry } from '@pnpm/fetching-types'
import ssri from 'ssri'

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

  // Fetch the full tarball to compute integrity hash
  const response = await fetchFromRegistry(wantedDependency.bareSpecifier)

  // Determine the resolved URL (follow redirects for immutable resources)
  let resolvedUrl: string
  if (response?.headers?.get('cache-control')?.includes('immutable')) {
    resolvedUrl = response.url
  } else {
    resolvedUrl = wantedDependency.bareSpecifier
  }

  // Read the tarball content and compute integrity
  const buffer = Buffer.from(await response.arrayBuffer())
  const integrity = ssri.fromData(buffer).toString()

  return {
    id: resolvedUrl as PkgResolutionId,
    normalizedBareSpecifier: resolvedUrl,
    resolution: {
      integrity,
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
