import { Dependencies, PackageManifest, ReadPackageHook } from '@pnpm/types'
import parseWantedDependency from '@pnpm/parse-wanted-dependency'
import semver from 'semver'

export default function (overrides: Record<string, string>): ReadPackageHook {
  const genericVersionOverrides = [] as VersionOverride[]
  const versionOverrides = [] as VersionOverrideWithParent[]
  Object.entries(overrides)
    .forEach(([selector, newPref]) => {
      if (selector.includes('>')) {
        const [parentSelector, childSelector] = selector.split('>')
        versionOverrides.push({
          newPref,
          parentWantedDependency: parseWantedDependency(parentSelector),
          wantedDependency: parseWantedDependency(childSelector),
        } as VersionOverrideWithParent)
        return
      }
      genericVersionOverrides.push({
        newPref,
        wantedDependency: parseWantedDependency(selector),
      } as VersionOverride)
    })
  return ((pkg: PackageManifest) => {
    overrideDepsOfPkg(pkg, versionOverrides.filter(({ parentWantedDependency }) => {
      return parentWantedDependency.alias === pkg.name && (
        !parentWantedDependency.pref || semver.satisfies(pkg.version, parentWantedDependency.pref)
      )
    }))
    overrideDepsOfPkg(pkg, genericVersionOverrides)
    return pkg
  }) as ReadPackageHook
}

interface VersionOverride {
  parentWantedDependency?: {
    alias: string
    pref?: string
  }
  wantedDependency: {
    alias: string
    pref?: string
  }
  newPref: string
}

interface VersionOverrideWithParent extends VersionOverride {
  parentWantedDependency: {
    alias: string
    pref?: string
  }
}

function overrideDepsOfPkg (pkg: PackageManifest, versionOverrides: VersionOverride[]) {
  if (pkg.dependencies != null) overrideDeps(versionOverrides, pkg.dependencies)
  if (pkg.optionalDependencies != null) overrideDeps(versionOverrides, pkg.optionalDependencies)
  return pkg
}

function overrideDeps (versionOverrides: VersionOverride[], deps: Dependencies) {
  for (const versionOverride of versionOverrides) {
    const actual = deps[versionOverride.wantedDependency.alias]
    if (
      actual &&
      (
        !versionOverride.wantedDependency.pref ||
        actual === versionOverride.wantedDependency.pref ||
        semver.validRange(actual) != null &&
        semver.validRange(versionOverride.wantedDependency.pref) != null &&
        semver.subset(actual, versionOverride.wantedDependency.pref)
      )
    ) {
      deps[versionOverride.wantedDependency.alias] = versionOverride.newPref
    }
  }
}
