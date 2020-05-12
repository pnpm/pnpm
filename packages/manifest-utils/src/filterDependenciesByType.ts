import { IncludedDependencies, ProjectManifest } from '@pnpm/types'

export function filterDependenciesByType (
  manifest: ProjectManifest,
  include: IncludedDependencies
) {
  return {
    ...(include.dependencies ? manifest.dependencies : {}),
    ...(include.devDependencies ? manifest.devDependencies : {}),
    ...(include.optionalDependencies ? manifest.optionalDependencies : {}),
  }
}
