import { Dependencies, PackageManifest, ReadPackageHook } from '@pnpm/types'
import parseWantedDependency from '@pnpm/parse-wanted-dependency'

export default function (resolutions: Record<string, string>): ReadPackageHook {
  const replacements = Object.entries(resolutions)
    .map(([rawWantedDependency, newPref]) => ({
      newPref,
      wantedDependency: parseWantedDependency(rawWantedDependency),
    } as VersionReplacement))
  return ((pkg: PackageManifest) => {
    if (pkg.dependencies) replaceDeps(replacements, pkg.dependencies)
    if (pkg.optionalDependencies) replaceDeps(replacements, pkg.optionalDependencies)
    return pkg
  }) as ReadPackageHook
}

interface VersionReplacement {
  wantedDependency: {
    alias: string
    pref?: string
  }
  newPref: string
}

function replaceDeps (replacements: VersionReplacement[], deps: Dependencies) {
  for (const replacement of replacements) {
    if (
      deps[replacement.wantedDependency.alias] &&
      (
        !replacement.wantedDependency.pref ||
        deps[replacement.wantedDependency.alias] === replacement.wantedDependency.pref
      )
    ) {
      deps[replacement.wantedDependency.alias] = replacement.newPref
    }
  }
}
