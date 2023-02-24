import semver from 'semver'
import isEmpty from 'ramda/src/isEmpty'
import { PeerDependencyRules, ReadPackageHook, PackageManifest, ProjectManifest } from '@pnpm/types'
import { PnpmError } from '@pnpm/error'
import { parseOverrides, VersionOverride } from '@pnpm/parse-overrides'
import { createMatcher } from '@pnpm/matcher'
import { isSubRange } from '.'

export function createPeerDependencyPatcher (
  peerDependencyRules: PeerDependencyRules
): ReadPackageHook {
  const ignoreMissingPatterns = [...new Set(peerDependencyRules.ignoreMissing ?? [])]
  const ignoreMissingMatcher = createMatcher(ignoreMissingPatterns)
  const allowAnyPatterns = [...new Set(peerDependencyRules.allowAny ?? [])]
  const allowAnyMatcher = createMatcher(allowAnyPatterns)
  const { allowedVersionsMatchAll, allowedVersionsByParentPkgName } = parseAllowedVersions(peerDependencyRules.allowedVersions ?? {})
  const _allowedVersionsByParentPkg = allowedVersionsByParentPkg.bind(null, {
    allowedVersionsMatchAll,
    allowedVersionsByParentPkgName,
  })

  return ((pkg) => {
    if (isEmpty(pkg.peerDependencies)) return pkg
    const allowedVersions = _allowedVersionsByParentPkg(pkg)
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
  let overrides: VersionOverride[]
  try {
    overrides = parseOverrides(allowedVersions ?? {})
  } catch (err) {
    throw new PnpmError('INVALID_ALLOWED_VERSION_SELECTOR',
      `${(err as PnpmError).message} in pnpm.peerDependencyRules.allowedVersions`)
  }
  const allowedVersionsMatchAll: Record<string, string[]> = {}
  const allowedVersionsByParentPkgName: AllowedVersionsByParentPkgName = {}
  for (const override of overrides) {
    if (!override.parentPkg) {
      allowedVersionsMatchAll[override.targetPkg.name] = parseVersions(override.newPref)
      continue
    }
    if (!allowedVersionsByParentPkgName[override.parentPkg.name]) {
      allowedVersionsByParentPkgName[override.parentPkg.name] = []
    }
    allowedVersionsByParentPkgName[override.parentPkg.name].push({
      parentPkg: override.parentPkg,
      targetPkg: override.targetPkg,
      ranges: parseVersions(override.newPref),
    })
  }
  return {
    allowedVersionsMatchAll,
    allowedVersionsByParentPkgName,
  }
}

function allowedVersionsByParentPkg (
  {
    allowedVersionsMatchAll,
    allowedVersionsByParentPkgName,
  }: {
    allowedVersionsMatchAll: Record<string, string[]>
    allowedVersionsByParentPkgName: AllowedVersionsByParentPkgName
  },
  pkg: PackageManifest | ProjectManifest
) {
  let allowedVersions = allowedVersionsMatchAll
  if (pkg.name && allowedVersionsByParentPkgName[pkg.name]) {
    allowedVersions = { ...allowedVersions }
    const pkgVer = pkg.version ?? ''
    for (const override of allowedVersionsByParentPkgName[pkg.name]) {
      if (!pkg.peerDependencies![override.targetPkg.name]) continue
      if (!override.parentPkg.pref ||
        (isSubRange(override.parentPkg.pref, pkgVer) || semver.satisfies(pkgVer, override.parentPkg.pref))) {
        allowedVersions[override.targetPkg.name] = override.ranges
      }
    }
  }
  return allowedVersions
}

function parseVersions (versions: string): string[] {
  return versions.split('||').map(v => v.trim())
}
