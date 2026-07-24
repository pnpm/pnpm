import type { Dependencies, DependenciesOrPeersField, ProjectManifest } from '@pnpm/types'

export function getAllDependenciesFromManifest (
  manifest: Pick<ProjectManifest, DependenciesOrPeersField>,
  opts?: { autoInstallPeers?: boolean }
): Dependencies {
  return {
    // The peer dependencies are spread first, so that the specifiers of
    // installed dependencies are not overridden if a dependency is present
    // both in `peerDependencies` and one of the other dependency fields.
    ...(opts?.autoInstallPeers ? manifest.peerDependencies : {}),
    ...manifest.devDependencies,
    ...manifest.dependencies,
    ...manifest.optionalDependencies,
  }
}
