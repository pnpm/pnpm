import semver from 'semver'
import isEmpty from 'ramda/src/isEmpty'
import { PeerDependencyRules, ReadPackageHook } from '@pnpm/types'
import { PnpmError } from '@pnpm/error'
import { parseOverrides } from '@pnpm/parse-overrides'
import { createMatcher } from '@pnpm/matcher'
import { isSubRange } from '.'

export function createPeerDependencyPatcher (
  peerDependencyRules: PeerDependencyRules
): ReadPackageHook {
  const ignoreMissingPatterns = [...new Set(peerDependencyRules.ignoreMissing ?? [])]
  const ignoreMissingMatcher = createMatcher(ignoreMissingPatterns)
  const allowAnyPatterns = [...new Set(peerDependencyRules.allowAny ?? [])]
  const allowAnyMatcher = createMatcher(allowAnyPatterns)

  let overrides: ReturnType<typeof parseOverrides>
  try {
    overrides = parseOverrides(peerDependencyRules.allowedVersions ?? {})
  } catch (e) {
    throw new PnpmError('INVALID_ALLOWED_VERSION_SELECTOR',
      `${(e as PnpmError).message} in pnpm.peerDependencyRules.allowedVersions`)
  }

  return ((pkg) => {
    if (isEmpty(pkg.peerDependencies)) return pkg
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
      const allowedVersions = parseVersions(peerDependencyRules.allowedVersions[peerName])
      const currentVersions = parseVersions(pkg.peerDependencies![peerName])

      allowedVersions.forEach(allowedVersion => {
        if (!currentVersions.includes(allowedVersion)) {
          currentVersions.push(allowedVersion)
        }
      })

      pkg.peerDependencies![peerName] = currentVersions.join(' || ')
    }

    const peerDepsOverrides = overrides.filter((override) => override.parentPkg && override.parentPkg.name === pkg.name)

    peerDepsOverrides.forEach(override => {
      const pkgVer = pkg.version ?? ''

      if (!override.parentPkg!.pref ||
        (isSubRange(override.parentPkg!.pref, pkgVer) || semver.satisfies(pkgVer, override.parentPkg!.pref))) {
        pkg.peerDependencies![override.targetPkg.name] = override.newPref
      }
    })

    return pkg
  }) as ReadPackageHook
}

function parseVersions (versions: string) {
  return versions.split('||').map(v => v.trim())
}
