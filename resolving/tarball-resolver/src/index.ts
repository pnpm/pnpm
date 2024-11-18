import { type PkgResolutionId, type ResolveResult } from '@pnpm/resolver-base'

export async function resolveFromTarball (
  wantedDependency: { pref: string }
): Promise<ResolveResult | null> {
  if (!wantedDependency.pref.startsWith('http:') && !wantedDependency.pref.startsWith('https:')) {
    return null
  }

  if (isRepository(wantedDependency.pref)) return null

  return {
    id: wantedDependency.pref as PkgResolutionId,
    normalizedPref: wantedDependency.pref,
    resolution: {
      tarball: wantedDependency.pref,
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
