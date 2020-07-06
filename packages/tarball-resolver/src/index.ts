import { ResolveResult } from '@pnpm/resolver-base'

const GIT_HOSTERS = new Set([
  'github.com',
  'gitlab.com',
  'bitbucket.org',
])

export default async function resolveTarball (
  wantedDependency: {pref: string}
): Promise<ResolveResult | null> {
  if (!wantedDependency.pref.startsWith('http:') && !wantedDependency.pref.startsWith('https:')) {
    return null
  }

  const parts = wantedDependency.pref.split('/')
  if (parts.length === 5 && GIT_HOSTERS.has(parts[2])) {
    return null
  }

  return {
    id: `@${wantedDependency.pref.replace(/^.*:\/\/(git@)?/, '')}`,
    normalizedPref: wantedDependency.pref,
    resolution: {
      tarball: wantedDependency.pref,
    },
    resolvedVia: 'url',
  }
}
