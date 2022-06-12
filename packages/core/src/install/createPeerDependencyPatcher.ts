import { PeerDependencyRules, ReadPackageHook, Dependencies } from '@pnpm/types'
import matcher from '@pnpm/matcher'
import isEmpty from 'ramda/src/isEmpty'

export default function (
  peerDependencyRules: PeerDependencyRules
): ReadPackageHook {
  const ignoreMissingPatterns = [...new Set(peerDependencyRules.ignoreMissing ?? [])]
  const ignoreMissingMatcher = matcher(ignoreMissingPatterns)
  const allowAnyPatterns = [...new Set(peerDependencyRules.allowAny ?? [])]
  return ((pkg) => {
    if (isEmpty(pkg.peerDependencies)) return pkg
    const allowAnyPeerDeps = getAllowAnyPeerDeps(allowAnyPatterns, pkg.peerDependencies ?? {})
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
      if (
        peerDependencyRules.allowedVersions?.[peerName] &&
        peerVersion !== '*'
      ) {
        if (peerDependencyRules.allowedVersions[peerName] === '*') {
          pkg.peerDependencies![peerName] = '*'
        } else {
          const allowedVersions = parseVersions(peerDependencyRules.allowedVersions[peerName])
          const currentVersions = parseVersions(pkg.peerDependencies![peerName])

          allowedVersions.forEach(allowedVersion => {
            if (!currentVersions.includes(allowedVersion)) {
              currentVersions.push(allowedVersion)
            }
          })

          pkg.peerDependencies![peerName] = currentVersions.join(' || ')
        }
      }
    }
    pkg.peerDependencies = { ...pkg.peerDependencies, ...allowAnyPeerDeps }
    return pkg
  }) as ReadPackageHook
}

function parseVersions (versions: string) {
  return versions.split('||').map(v => v.trim())
}

function getAllowAnyPeerDeps (allowAnyPatterns: string[], peerDependencies: Dependencies) {
  const allowAnyMatcher = matcher(allowAnyPatterns)
  const allowAnyPeerDeps = {}
  Object.keys(peerDependencies ?? {}).forEach(peerDependency => {
    if (allowAnyMatcher(peerDependency)) {
      allowAnyPeerDeps[peerDependency] = '*'
    }
  })
  return allowAnyPeerDeps
}