import {
  hasDirective,
  loadTarballResolution,
  tarballFreshness,
} from '@pnpm/fetching.tarball-fetcher'
import type { FetchFromRegistry } from '@pnpm/fetching.types'
import type { LatestInfo, LatestQuery, PkgResolutionId, ResolveResult, TarballResolution } from '@pnpm/resolving.resolver-base'

export type {
  TarballFreshness,
  TarballResolutionRecord,
} from '@pnpm/fetching.tarball-fetcher'
export {
  loadTarballResolution,
  removeTarballResolution,
  storeTarballResolution,
  tarballFreshness,
} from '@pnpm/fetching.tarball-fetcher'

export interface TarballResolveResult extends ResolveResult {
  normalizedBareSpecifier: string
  resolution: TarballResolution
  resolvedVia: 'url'
}

export async function resolveFromTarball (
  fetchFromRegistry: FetchFromRegistry,
  wantedDependency: { bareSpecifier: string },
  opts?: { cacheDir?: string, getAuthHeader?: (url: string) => string | undefined }
): Promise<TarballResolveResult | null> {
  if (!wantedDependency.bareSpecifier.startsWith('http:') && !wantedDependency.bareSpecifier.startsWith('https:')) {
    return null
  }

  // The URL is normalized to remove the port if it is the default port of the protocol.
  const normalizedBareSpecifier = new URL(wantedDependency.bareSpecifier).toString()
  const cacheDir = opts?.getAuthHeader?.(normalizedBareSpecifier) ? undefined : opts?.cacheDir
  const cached = cacheDir ? loadTarballResolution(cacheDir, normalizedBareSpecifier) : undefined
  const freshness = cached ? tarballFreshness(cached) : undefined
  if (cached && freshness === 'fresh') {
    return tarballResult(normalizedBareSpecifier, cached.tarball, cached.integrity)
  }
  // The fetcher revalidates a stale etag with If-None-Match, so skip the resolve HEAD.
  if (cached?.etag && freshness === 'revalidate') {
    return tarballResult(normalizedBareSpecifier, cached.tarball)
  }
  let resolvedUrl: string

  // If there are redirects and the response is immutable, we want to get the final URL address
  const response = await fetchFromRegistry(normalizedBareSpecifier, { method: 'HEAD' })
  const cacheControl = response?.headers?.get('cache-control')
  if (cacheControl && hasDirective(cacheControl, 'immutable')) {
    resolvedUrl = response.url
  } else {
    resolvedUrl = normalizedBareSpecifier
  }

  return tarballResult(normalizedBareSpecifier, resolvedUrl)
}

function tarballResult (normalizedBareSpecifier: string, tarball: string, integrity?: string): TarballResolveResult {
  const resolution: TarballResolution = integrity ? { tarball, integrity } : { tarball }
  return {
    id: normalizedBareSpecifier as PkgResolutionId,
    normalizedBareSpecifier,
    resolution,
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
