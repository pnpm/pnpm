import { type Dependencies, type IncludedDependencies, type ProjectManifest } from '@pnpm/types'

export function filterDependenciesByType (
  manifest: ProjectManifest,
  include: IncludedDependencies,
  opts?: { autoInstallPeers?: boolean }
): Dependencies {
  return {
    // Peers are spread first so explicit deps/devDeps/optionalDeps override peer ranges
    ...(opts?.autoInstallPeers ? manifest.peerDependencies : {}),
    ...(include.devDependencies ? manifest.devDependencies : {}),
    ...(include.dependencies ? manifest.dependencies : {}),
    ...(include.optionalDependencies ? manifest.optionalDependencies : {}),
  }
}
