import type { Dependencies, IncludedDependencies, ProjectManifest } from '@pnpm/types'

export function filterDependenciesByType (
  manifest: ProjectManifest,
  include: IncludedDependencies
): Dependencies {
  return {
    ...(include.peerDependencies ? manifest.peerDependencies : {}),
    ...(include.devDependencies ? manifest.devDependencies : {}),
    ...(include.dependencies ? manifest.dependencies : {}),
    ...(include.optionalDependencies ? manifest.optionalDependencies : {}),
  }
}
