import path from 'path'
import partition from 'ramda/src/partition.js'
import { Dependencies, PackageManifest, ReadPackageHook } from '@pnpm/types'
import parseOverrides from '@pnpm/parse-overrides'
import normalizePath from 'normalize-path'
import semver from 'semver'

export default function (
  overrides: Record<string, string>,
  rootDir: string
): ReadPackageHook {
  const [versionOverrides, genericVersionOverrides] = partition(({ parentPkg }) => parentPkg != null,
    parseOverrides(overrides)
      .map((override) => {
        let linkTarget: string | undefined
        if (override.newPref.startsWith('link:')) {
          linkTarget = path.join(rootDir, override.newPref.substring(5))
        }
        return {
          ...override,
          linkTarget,
        }
      })
  ) as [VersionOverrideWithParent[], VersionOverride[]]
  return ((manifest: PackageManifest, dir?: string) => {
    overrideDepsOfPkg({ manifest, dir }, versionOverrides.filter(({ parentPkg }) => {
      return parentPkg.name === manifest.name && (
        !parentPkg.pref || semver.satisfies(manifest.version, parentPkg.pref)
      )
    }))
    overrideDepsOfPkg({ manifest, dir }, genericVersionOverrides)
    return manifest
  }) as ReadPackageHook
}

interface VersionOverride {
  parentPkg?: {
    name: string
    pref?: string
  }
  targetPkg: {
    name: string
    pref?: string
  }
  newPref: string
  linkTarget?: string
}

interface VersionOverrideWithParent extends VersionOverride {
  parentPkg: {
    name: string
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
    const actual = deps[versionOverride.targetPkg.name]
    if (actual == null) continue
    if (!isSubRange(versionOverride.targetPkg.pref, actual)) continue
    if (versionOverride.linkTarget && dir) {
      deps[versionOverride.targetPkg.name] = `link:${normalizePath(path.relative(dir, versionOverride.linkTarget))}`
      continue
    }
    deps[versionOverride.targetPkg.name] = versionOverride.newPref
  }
}

function isSubRange (superRange: string | undefined, subRange: string) {
  return !superRange ||
  subRange === superRange ||
  semver.validRange(subRange) != null &&
  semver.validRange(superRange) != null &&
  semver.subset(subRange, superRange)
}
