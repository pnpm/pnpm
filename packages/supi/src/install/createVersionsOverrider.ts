import { Dependencies, PackageManifest, ReadPackageHook } from '@pnpm/types'
import parseWantedDependency from '@pnpm/parse-wanted-dependency'

export default function (overrides: Record<string, string>): ReadPackageHook {
  const versionOverrides = Object.entries(overrides)
    .map(([rawWantedDependency, newPref]) => ({
      newPref,
      wantedDependency: parseWantedDependency(rawWantedDependency),
    } as VersionOverride))
  return ((pkg: PackageManifest) => {
    if (pkg.dependencies) overrideDeps(versionOverrides, pkg.dependencies)
    if (pkg.optionalDependencies) overrideDeps(versionOverrides, pkg.optionalDependencies)
    return pkg
  }) as ReadPackageHook
}

interface VersionOverride {
  wantedDependency: {
    alias: string
    pref?: string
  }
  newPref: string
}

function overrideDeps (versionOverrides: VersionOverride[], deps: Dependencies) {
  for (const versionOverride of versionOverrides) {
    if (
      deps[versionOverride.wantedDependency.alias] &&
      (
        !versionOverride.wantedDependency.pref ||
        deps[versionOverride.wantedDependency.alias] === versionOverride.wantedDependency.pref
      )
    ) {
      deps[versionOverride.wantedDependency.alias] = versionOverride.newPref
    }
  }
}
