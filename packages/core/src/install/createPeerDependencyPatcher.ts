import { PeerDependencyRules, ReadPackageHook } from '@pnpm/types'
import matcher from '@pnpm/matcher'
import isEmpty from 'ramda/src/isEmpty'

export default function (
  peerDependencyRules: PeerDependencyRules
): ReadPackageHook {
  const ignoreMissingPatterns = [...new Set(peerDependencyRules.ignoreMissing ?? [])]
  const ignoreMissingMatcher = matcher(ignoreMissingPatterns)
  const allowAnyPatterns = [...new Set(peerDependencyRules.allowAny ?? [])]
  const allowAnyMatcher = matcher(allowAnyPatterns)
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
    return pkg
  }) as ReadPackageHook
}

function parseVersions (versions: string) {
  return versions.split('||').map(v => v.trim())
}
