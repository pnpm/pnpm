import path from 'path'
import { Dependencies, PackageManifest, ReadPackageHook } from '@pnpm/types'
import parseWantedDependency from '@pnpm/parse-wanted-dependency'
import normalizePath from 'normalize-path'
import semver from 'semver'

export default function (
  overrides: Record<string, string>,
  rootDir: string
): ReadPackageHook {
  const genericVersionOverrides = [] as VersionOverride[]
  const versionOverrides = [] as VersionOverrideWithParent[]
  Object.entries(overrides)
    .forEach(([selector, newPref]) => {
      let linkTarget: string | undefined
      if (newPref.startsWith('link:')) {
        linkTarget = path.join(rootDir, newPref.substring(5))
      }
      if (selector.includes('>') && (!selector.includes('@') || selector.indexOf('>') < selector.lastIndexOf('@'))) {
        const delimiterIndex = selector.indexOf('>')
        const parentSelector = selector.substring(0, delimiterIndex)
        const childSelector = selector.substring(delimiterIndex + 1)
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
  return ((manifest: PackageManifest, dir?: string) => {
    overrideDepsOfPkg({ manifest, dir }, versionOverrides.filter(({ parentWantedDependency }) => {
      return parentWantedDependency.alias === manifest.name && (
        !parentWantedDependency.pref || semver.satisfies(manifest.version, parentWantedDependency.pref)
      )
    }))
    overrideDepsOfPkg({ manifest, dir }, genericVersionOverrides)
    return manifest
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

function overrideDepsOfPkg (
  { manifest, dir }: { manifest: PackageManifest, dir: string | undefined },
  versionOverrides: VersionOverride[]
) {
  if (manifest.dependencies != null) overrideDeps(versionOverrides, manifest.dependencies, dir)
  if (manifest.optionalDependencies != null) overrideDeps(versionOverrides, manifest.optionalDependencies, dir)
  if (manifest.devDependencies != null) overrideDeps(versionOverrides, manifest.devDependencies, dir)
  return manifest
}

function overrideDeps (versionOverrides: VersionOverride[], deps: Dependencies, dir: string | undefined) {
  for (const versionOverride of versionOverrides) {
    const actual = deps[versionOverride.wantedDependency.alias]
    if (actual == null) continue
    if (!isSubRange(versionOverride.wantedDependency.pref, actual)) continue
    if (versionOverride.linkTarget && dir) {
      deps[versionOverride.wantedDependency.alias] = `link:${normalizePath(path.relative(dir, versionOverride.linkTarget))}`
      continue
    }
    deps[versionOverride.wantedDependency.alias] = versionOverride.newPref
  }
}

function isSubRange (superRange: string | undefined, subRange: string) {
  return !superRange ||
  subRange === superRange ||
  semver.validRange(subRange) != null &&
  semver.validRange(superRange) != null &&
  semver.subset(subRange, superRange)
}
