import type { Dependencies, DependenciesOrPeersField, ProjectManifest } from '@pnpm/types'

export function getAllDependenciesFromManifest (
  manifest: Pick<ProjectManifest, DependenciesOrPeersField>,
  opts?: { autoInstallPeers?: boolean }
): Dependencies {
  return {
    ...(opts?.autoInstallPeers ? manifest.peerDependencies : {}),
    ...manifest.devDependencies,
    ...manifest.dependencies,
    ...manifest.optionalDependencies,
  }
}
