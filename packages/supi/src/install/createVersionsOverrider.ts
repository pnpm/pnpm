import path from 'path'
import { Dependencies, PackageManifest, ReadPackageHook } from '@pnpm/types'
import parseWantedDependency from '@pnpm/parse-wanted-dependency'
import normalizePath from 'normalize-path'
import semver from 'semver'

export default function (overrides: Record<string, string>, rootDir: string): ReadPackageHook {
  const genericVersionOverrides = [] as VersionOverride[]
  const versionOverrides = [] as VersionOverrideWithParent[]
  Object.entries(overrides)
    .forEach(([selector, newPref]) => {
      let linkTarget: string | undefined
      if (newPref.startsWith('link:')) {
        linkTarget = path.join(rootDir, newPref.substring(5))
      }
      if (selector.includes('>')) {
        const [parentSelector, childSelector] = selector.split('>')
        versionOverrides.push({
          linkTarget,
          newPref,
          parentWantedDependency: parseWantedDependency(parentSelector),
          wantedDependency: parseWantedDependency(childSelector),
        } as VersionOverrideWithParent)
        return
      }
      genericVersionOverrides.push({
        linkTarget,
        newPref,
        wantedDependency: parseWantedDependency(selector),
      } as VersionOverride)
    })
  return ((pkg: PackageManifest, dir?: string) => {
    overrideDepsOfPkg(pkg, dir, versionOverrides.filter(({ parentWantedDependency }) => {
      return parentWantedDependency.alias === pkg.name && (
        !parentWantedDependency.pref || semver.satisfies(pkg.version, parentWantedDependency.pref)
      )
    }))
    overrideDepsOfPkg(pkg, dir, genericVersionOverrides)
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
  linkTarget?: string
}

interface VersionOverrideWithParent extends VersionOverride {
  parentWantedDependency: {
    alias: string
    pref?: string
  }
}

function overrideDepsOfPkg (pkg: PackageManifest, dir: string | undefined, versionOverrides: VersionOverride[]) {
  if (pkg.dependencies != null) overrideDeps(versionOverrides, pkg.dependencies, dir)
  if (pkg.optionalDependencies != null) overrideDeps(versionOverrides, pkg.optionalDependencies, dir)
  if (pkg.devDependencies != null) overrideDeps(versionOverrides, pkg.devDependencies, dir)
  return pkg
}

function overrideDeps (versionOverrides: VersionOverride[], deps: Dependencies, dir: string | undefined) {
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
      if (versionOverride.linkTarget && dir) {
        deps[versionOverride.wantedDependency.alias] = `link:${normalizePath(path.relative(dir, versionOverride.linkTarget))}`
      } else {
        deps[versionOverride.wantedDependency.alias] = versionOverride.newPref
      }
    }
  }
}
