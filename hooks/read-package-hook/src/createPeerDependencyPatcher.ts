import semver from 'semver'
import isEmpty from 'ramda/src/isEmpty'
import { PeerDependencyRules, ReadPackageHook } from '@pnpm/types'
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

  let overrides: VersionOverride[]
  try {
    overrides = parseOverrides(peerDependencyRules.allowedVersions ?? {})
  } catch (err) {
    throw new PnpmError('INVALID_ALLOWED_VERSION_SELECTOR',
      `${(err as PnpmError).message} in pnpm.peerDependencyRules.allowedVersions`)
  }
  const overridesByParentPkgName = overrides.reduce((acc, override) => {
    if (!override.parentPkg) return acc
    if (!acc[override.parentPkg.name]) {
      acc[override.parentPkg.name] = []
    }
    acc[override.parentPkg.name].push(override as Required<VersionOverride>)
    return acc
  }, {} as Record<string, Array<Required<VersionOverride>>>)

  return ((pkg) => {
    if (isEmpty(pkg.peerDependencies)) return pkg
    let allowedVersionsRaw = peerDependencyRules.allowedVersions ?? {}
    if (pkg.name && overridesByParentPkgName[pkg.name]) {
      allowedVersionsRaw = { ...allowedVersionsRaw }
      const pkgVer = pkg.version ?? ''
      for (const override of overridesByParentPkgName[pkg.name]) {
        if (!pkg.peerDependencies![override.targetPkg.name]) continue
        if (!override.parentPkg.pref ||
          (isSubRange(override.parentPkg.pref, pkgVer) || semver.satisfies(pkgVer, override.parentPkg.pref))) {
          allowedVersionsRaw[override.targetPkg.name] = override.newPref
        }
      }
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
      const allowedVersions = parseVersions(allowedVersionsRaw[peerName])
      const currentVersions = parseVersions(pkg.peerDependencies![peerName])

      allowedVersions.forEach(allowedVersion => {
        if (!currentVersions.includes(allowedVersion)) {
          currentVersions.push(allowedVersion)
        }
      })

      pkg.peerDependencies![peerName] = currentVersions.join(' || ')
    }

    return pkg
  }) as ReadPackageHook
}

function parseVersions (versions: string) {
  return versions.split('||').map(v => v.trim())
}
