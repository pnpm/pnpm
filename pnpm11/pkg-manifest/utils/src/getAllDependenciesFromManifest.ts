import type { Dependencies, DependenciesOrPeersField, ProjectManifest } from '@pnpm/types'

export function getAllDependenciesFromManifest (
  manifest: Pick<ProjectManifest, DependenciesOrPeersField>,
  opts?: { autoInstallPeers?: boolean, peerAliases?: Set<string> }
): Dependencies {
  const selectedPeerDependencies = opts?.peerAliases == null
    ? {}
    : Object.fromEntries(Object.entries(manifest.peerDependencies ?? {}).filter(([alias]) => opts.peerAliases!.has(alias)))
  return {
    ...(opts?.autoInstallPeers ? manifest.peerDependencies : selectedPeerDependencies),
    ...manifest.devDependencies,
    ...manifest.dependencies,
    ...manifest.optionalDependencies,
  }
}
