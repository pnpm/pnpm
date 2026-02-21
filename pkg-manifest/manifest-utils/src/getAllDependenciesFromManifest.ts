import { type Dependencies, type DependenciesField, type ProjectManifest } from '@pnpm/types'

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
