import type { Dependencies, DependenciesField, ProjectManifest } from '@pnpm/types'

export function getAllDependenciesFromManifest (
  pkg: Pick<ProjectManifest, DependenciesField | 'peerDependencies'>,
  opts?: { autoInstallPeers?: boolean }
): Dependencies {
  return {
    ...pkg.devDependencies,
    ...pkg.dependencies,
    ...pkg.optionalDependencies,
    ...(opts?.autoInstallPeers ? pkg.peerDependencies : {}),
  } as Dependencies
}
