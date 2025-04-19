import { type PkgResolutionId, type ResolveResult } from '@pnpm/resolver-base'
import { type FetchFromRegistry, type FetchFromRegistryOptions } from '@pnpm/fetching-types'

export async function resolveFromTarball (
  fetchFromRegistry: FetchFromRegistry,
  wantedDependency: { pref: string }
): Promise<ResolveResult | null> {
  if (!wantedDependency.pref.startsWith('http:') && !wantedDependency.pref.startsWith('https:')) {
    return null
  }

  if (isRepository(wantedDependency.pref)) return null

  // If there are redirects, we want to get the final URL address
  const opts: FetchFromRegistryOptions = {
    method: 'HEAD',
  }
  const _url = new URL(wantedDependency.pref)
  if (_url.hostname === 'pkg.pr.new') {
    const controller = new AbortController()
    opts.method = 'GET' // pkg.pr.new don't support HEAD requests
    opts.abort = controller.abort.bind(controller)
    opts.signal = controller.signal
  }
  const { url: resolvedUrl } = await fetchFromRegistry(wantedDependency.pref, opts)

  return {
    id: resolvedUrl as PkgResolutionId,
    specifier: resolvedUrl,
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

function isRepository (pref: string): boolean {
  const url = new URL(pref)
  if (url.hash && url.hash.includes('/')) {
    url.hash = encodeURIComponent(url.hash.substring(1))
    pref = url.href
  }
  if (pref.endsWith('/')) {
    pref = pref.slice(0, -1)
  }
  const parts = pref.split('/')
  return (parts.length === 5 && GIT_HOSTERS.has(parts[2]))
}
