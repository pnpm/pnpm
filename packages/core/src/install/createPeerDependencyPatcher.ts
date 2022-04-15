import { PeerDependencyRules, ReadPackageHook } from '@pnpm/types'
import isEmpty from 'ramda/src/isEmpty'

export default function (
  peerDependencyRules: PeerDependencyRules
): ReadPackageHook {
  const ignoreMissing = new Set(peerDependencyRules.ignoreMissing ?? [])
  return ((pkg) => {
    if (isEmpty(pkg.peerDependencies)) return pkg
    for (const [peerName, peerVersion] of Object.entries(pkg.peerDependencies ?? {})) {
      if (ignoreMissing.has(peerName) && !pkg.peerDependenciesMeta?.[peerName]?.optional) {
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
    return pkg
  }) as ReadPackageHook
}

function parseVersions (versions: string) {
  return versions.split('||').map(v => v.trim())
}
