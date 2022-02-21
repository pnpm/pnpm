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
          pkg.peerDependencies![peerName] += ` || ${peerDependencyRules.allowedVersions[peerName]}`
        }
      }
    }
    return pkg
  }) as ReadPackageHook
}
