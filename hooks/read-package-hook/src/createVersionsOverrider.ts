import path from 'node:path'
import semver from 'semver'
import partition from 'ramda/src/partition'
import type {
  Dependencies,
  PackageManifest,
  ProjectManifest,
  ReadPackageHook,
} from '@pnpm/types'
import { PnpmError } from '@pnpm/error'
import { parseOverrides } from '@pnpm/parse-overrides'
import normalizePath from 'normalize-path'
import { isIntersectingRange } from './isIntersectingRange'

export function createVersionsOverrider(
  overrides: Record<string, string>,
  rootDir: string
): ReadPackageHook {
  const parsedOverrides = tryParseOverrides(overrides)

  const [versionOverrides, genericVersionOverrides] = partition(
    ({ parentPkg }): boolean => {
      return parentPkg != null;
    },
    parsedOverrides.map((override: VersionOverride): VersionOverride => {
      let linkTarget: string | undefined

      if (override.newPref.startsWith('link:')) {
        linkTarget = path.join(rootDir, override.newPref.substring(5))
      }

      let linkFileTarget: string | undefined

      if (override.newPref.startsWith('file:')) {
        const pkgPath = override.newPref.substring(5)

        linkFileTarget = path.isAbsolute(pkgPath)
          ? pkgPath
          : path.join(rootDir, pkgPath)
      }

      return {
        ...override,
        linkTarget,
        linkFileTarget,
      }
    })
  )

  return (manifest?: PackageManifest | ProjectManifest | undefined, dir?: string | undefined): PackageManifest | ProjectManifest | undefined => {
    const versionOverridesWithParent = versionOverrides.filter(
      ({ parentPkg }: VersionOverride): boolean => {
        return (
          parentPkg?.name === manifest?.name &&
          (!parentPkg?.pref ||
            semver.satisfies(manifest?.version ?? '', parentPkg.pref))
        )
      }
    )
    overrideDepsOfPkg(
      { manifest, dir },
      versionOverridesWithParent,
      genericVersionOverrides
    )

    return manifest
  }
}

function tryParseOverrides(overrides: Record<string, string>): VersionOverride[] {
  try {
    return parseOverrides(overrides)
  } catch (e) {
    throw new PnpmError(
      'INVALID_OVERRIDES_SELECTOR',
      `${(e as PnpmError).message} in pnpm.overrides`
    )
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

function overrideDepsOfPkg(
  { manifest, dir }: { manifest: PackageManifest | ProjectManifest | undefined; dir: string | undefined },
  versionOverrides: VersionOverrideWithParent[] | VersionOverride[],
  genericVersionOverrides: VersionOverride[]
) {
  if (typeof manifest?.dependencies !== 'undefined')
    overrideDeps(
      versionOverrides,
      genericVersionOverrides,
      manifest.dependencies,
      dir
    )
  if (typeof manifest?.optionalDependencies !== 'undefined')
    overrideDeps(
      versionOverrides,
      genericVersionOverrides,
      manifest.optionalDependencies,
      dir
    )
  if (typeof manifest?.devDependencies !== 'undefined')
    overrideDeps(
      versionOverrides,
      genericVersionOverrides,
      manifest.devDependencies,
      dir
    )
  if (typeof manifest?.peerDependencies !== 'undefined')
    overrideDeps(
      versionOverrides,
      genericVersionOverrides,
      manifest.peerDependencies,
      dir
    )
}

function overrideDeps(
  versionOverrides: VersionOverrideWithParent[] | VersionOverride[],
  genericVersionOverrides: VersionOverride[],
  deps: Dependencies,
  dir: string | undefined
) {
  for (const [name, pref] of Object.entries(deps)) {
    const versionOverride =
      pickMostSpecificVersionOverride(
        versionOverrides.filter(
          ({ targetPkg }) =>
            targetPkg.name === name && isIntersectingRange(targetPkg.pref, pref)
        )
      ) ??
      pickMostSpecificVersionOverride(
        genericVersionOverrides.filter(
          ({ targetPkg }) =>
            targetPkg.name === name && isIntersectingRange(targetPkg.pref, pref)
        )
      )
    if (!versionOverride) continue

    if (versionOverride.linkTarget && dir) {
      deps[versionOverride.targetPkg.name] = `link:${normalizePath(
        path.relative(dir, versionOverride.linkTarget)
      )}`
      continue
    }
    if (versionOverride.linkFileTarget) {
      deps[versionOverride.targetPkg.name] =
        `file:${versionOverride.linkFileTarget}`
      continue
    }
    deps[versionOverride.targetPkg.name] = versionOverride.newPref
  }
}

function pickMostSpecificVersionOverride(
  versionOverrides: VersionOverride[]
): VersionOverride | undefined {
  return versionOverrides.sort((a, b) =>
    isIntersectingRange(b.targetPkg.pref ?? '', a.targetPkg.pref ?? '') ? -1 : 1
  )[0]
}
