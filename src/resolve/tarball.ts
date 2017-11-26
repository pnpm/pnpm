import parseNpmTarballUrl from 'parse-npm-tarball-url'
import {
  ResolveOptions,
  ResolveResult,
  TarballResolution,
  WantedDependency,
} from '.'

export default async function resolveTarball (
  wantedDependency: WantedDependency,
  opts: ResolveOptions,
): Promise<ResolveResult | null> {
  if (!wantedDependency.pref.startsWith('http:') && !wantedDependency.pref.startsWith('https:')) {
    return null
  }

  const resolution: TarballResolution = {
    tarball: wantedDependency.pref,
  }

  if (wantedDependency.pref.startsWith('http://registry.npmjs.org/')) {
    const parsed = parseNpmTarballUrl(wantedDependency.pref)
    if (parsed) {
      return {
        id: `${parsed.host}/${parsed.pkg.name}/${parsed.pkg.version}`,
        normalizedPref: wantedDependency.pref,
        resolution,
      }
    }
  }

  return {
    id: wantedDependency.pref
      .replace(/^.*:\/\/(git@)?/, '')
      .replace(/\.tgz$/, ''),
    normalizedPref: wantedDependency.pref,
    resolution,
  }
}
