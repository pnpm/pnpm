import { Dependencies, DependenciesField, ProjectManifest } from '@pnpm/types'

export function getAllDependenciesFromManifest (
  pkg: Pick<ProjectManifest, DependenciesField>
): Dependencies {
  return {
    ...pkg.devDependencies,
    ...pkg.dependencies,
    ...pkg.optionalDependencies,
  } as Dependencies
}
