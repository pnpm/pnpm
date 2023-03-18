import path from 'path'
import semver from 'semver'
import partition from 'ramda/src/partition'
import { type Dependencies, type PackageManifest, type ReadPackageHook } from '@pnpm/types'
import { PnpmError } from '@pnpm/error'
import { parseOverrides } from '@pnpm/parse-overrides'
import normalizePath from 'normalize-path'
import { isSubRange } from './isSubRange'

export function createVersionsOverrider (
  overrides: Record<string, string>,
  rootDir: string
): ReadPackageHook {
  const parsedOverrides = tryParseOverrides(overrides)
  const [versionOverrides, genericVersionOverrides] = partition(({ parentPkg }) => parentPkg != null,
    parsedOverrides
      .map((override) => {
        let linkTarget: string | undefined
        if (override.newPref.startsWith('link:')) {
          linkTarget = path.join(rootDir, override.newPref.substring(5))
        }
        let linkFileTarget: string | undefined
        if (override.newPref.startsWith('file:')) {
          const pkgPath = override.newPref.substring(5)
          linkFileTarget = path.isAbsolute(pkgPath) ? pkgPath : path.join(rootDir, pkgPath)
        }
        return {
          ...override,
          linkTarget,
          linkFileTarget,
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

function tryParseOverrides (overrides: Record<string, string>) {
  try {
    return parseOverrides(overrides)
  } catch (e) {
    throw new PnpmError('INVALID_OVERRIDES_SELECTOR', `${(e as PnpmError).message} in pnpm.overrides`)
  }
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
  linkFileTarget?: string
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
    if (versionOverride.linkFileTarget) {
      deps[versionOverride.targetPkg.name] = `file:${versionOverride.linkFileTarget}`
      continue
    }
    deps[versionOverride.targetPkg.name] = versionOverride.newPref
  }
}
