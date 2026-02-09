import { type Dependencies, type DependenciesOrPeersField, type ProjectManifest } from '@pnpm/types'

export function getAllDependenciesFromManifest (
  pkg: Pick<ProjectManifest, DependenciesOrPeersField>,
  opts?: { autoInstallPeers?: boolean }
): Dependencies {
  return {
    // Peers are spread first so explicit deps/devDeps/optionalDeps override peer ranges
    ...(opts?.autoInstallPeers ? pkg.peerDependencies : {}),
    ...pkg.devDependencies,
    ...pkg.dependencies,
    ...pkg.optionalDependencies,
  } as Dependencies
}
