import semver from 'semver'
import isEmpty from 'ramda/src/isEmpty'
import { PeerDependencyRules, ReadPackageHook, PackageManifest, ProjectManifest } from '@pnpm/types'
import { PnpmError } from '@pnpm/error'
import { parseOverrides, VersionOverride } from '@pnpm/parse-overrides'
import { createMatcher } from '@pnpm/matcher'
import { isSubRange } from './isSubRange'

export function createPeerDependencyPatcher (
  peerDependencyRules: PeerDependencyRules
): ReadPackageHook {
  const ignoreMissingPatterns = [...new Set(peerDependencyRules.ignoreMissing ?? [])]
  const ignoreMissingMatcher = createMatcher(ignoreMissingPatterns)
  const allowAnyPatterns = [...new Set(peerDependencyRules.allowAny ?? [])]
  const allowAnyMatcher = createMatcher(allowAnyPatterns)
  const { allowedVersionsMatchAll, allowedVersionsByParentPkgName } = parseAllowedVersions(peerDependencyRules.allowedVersions ?? {})
  const _getAllowedVersionsByParentPkg = getAllowedVersionsByParentPkg.bind(null, allowedVersionsByParentPkgName)

  return ((pkg) => {
    if (isEmpty(pkg.peerDependencies)) return pkg
    const allowedVersions = {
      ...allowedVersionsMatchAll,
      ..._getAllowedVersionsByParentPkg(pkg),
    }
    for (const [peerName, peerVersion] of Object.entries(pkg.peerDependencies ?? {})) {
      if (
        ignoreMissingMatcher(peerName) &&
        !pkg.peerDependenciesMeta?.[peerName]?.optional
      ) {
        pkg.peerDependenciesMeta = pkg.peerDependenciesMeta ?? {}
        pkg.peerDependenciesMeta[peerName] = {
          optional: true,
        }
      }
      if (allowAnyMatcher(peerName)) {
        pkg.peerDependencies![peerName] = '*'
        continue
      }
      if (
        !peerDependencyRules.allowedVersions?.[peerName] ||
        peerVersion === '*'
      ) continue
      if (peerDependencyRules.allowedVersions[peerName] === '*') {
        pkg.peerDependencies![peerName] = '*'
        continue
      }
      const currentVersions = parseVersions(pkg.peerDependencies![peerName])

      allowedVersions[peerName].forEach(allowedVersion => {
        if (!currentVersions.includes(allowedVersion)) {
          currentVersions.push(allowedVersion)
        }
      })

      pkg.peerDependencies![peerName] = currentVersions.join(' || ')
    }
    return pkg
  }) as ReadPackageHook
}

type AllowedVersionsByParentPkgName = Record<string, Array<Required<Pick<VersionOverride, 'parentPkg' | 'targetPkg'>> & { ranges: string[] }>>

function parseAllowedVersions (allowedVersions: Record<string, string>) {
  const overrides = tryParseAllowedVersions(allowedVersions)
  const allowedVersionsMatchAll: Record<string, string[]> = {}
  const allowedVersionsByParentPkgName: AllowedVersionsByParentPkgName = {}
  for (const { parentPkg, targetPkg, newPref } of overrides) {
    const ranges = parseVersions(newPref)
    if (!parentPkg) {
      allowedVersionsMatchAll[targetPkg.name] = ranges
      continue
    }
    if (!allowedVersionsByParentPkgName[parentPkg.name]) {
      allowedVersionsByParentPkgName[parentPkg.name] = []
    }
    allowedVersionsByParentPkgName[parentPkg.name].push({
      parentPkg,
      targetPkg,
      ranges,
    })
  }
  return {
    allowedVersionsMatchAll,
    allowedVersionsByParentPkgName,
  }
}

function tryParseAllowedVersions (allowedVersions: Record<string, string>): VersionOverride[] {
  try {
    return parseOverrides(allowedVersions ?? {})
  } catch (err) {
    throw new PnpmError('INVALID_ALLOWED_VERSION_SELECTOR',
      `${(err as PnpmError).message} in pnpm.peerDependencyRules.allowedVersions`)
  }
}

function getAllowedVersionsByParentPkg (
  allowedVersionsByParentPkgName: AllowedVersionsByParentPkgName,
  pkg: PackageManifest | ProjectManifest
): Record<string, string[]> {
  if (!pkg.name || !allowedVersionsByParentPkgName[pkg.name]) return {}

  return allowedVersionsByParentPkgName[pkg.name]
    .reduce((acc, { targetPkg, parentPkg, ranges }) => {
      if (!pkg.peerDependencies![targetPkg.name]) return acc
      if (!parentPkg.pref || pkg.version &&
        (isSubRange(parentPkg.pref, pkg.version) || semver.satisfies(pkg.version, parentPkg.pref))) {
        acc[targetPkg.name] = ranges
      }
      return acc
    }, {} as Record<string, string[]>)
}

function parseVersions (versions: string): string[] {
  return versions.split('||').map(v => v.trim())
}
