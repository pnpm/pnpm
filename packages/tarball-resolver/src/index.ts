import { ResolveResult } from '@pnpm/resolver-base'

export default async function resolveTarball (
  wantedDependency: {pref: string}
): Promise<ResolveResult | null> {
  if (!wantedDependency.pref.startsWith('http:') && !wantedDependency.pref.startsWith('https:')) {
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
