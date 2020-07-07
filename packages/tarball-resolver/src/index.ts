import { ResolveResult } from '@pnpm/resolver-base'

export default async function resolveTarball (
  wantedDependency: {pref: string}
): Promise<ResolveResult | null> {
  if (!wantedDependency.pref.startsWith('http:') && !wantedDependency.pref.startsWith('https:')) {
    return null
  }

  if (isRepository(wantedDependency.pref)) return null

  return {
    id: `@${wantedDependency.pref.replace(/^.*:\/\/(git@)?/, '')}`,
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

function isRepository (pref: string) {
  if (pref.endsWith('/')) {
    pref = pref.substr(0, pref.length - 1)
  }
  const parts = pref.split('/')
  return (parts.length === 5 && GIT_HOSTERS.has(parts[2]))
}
