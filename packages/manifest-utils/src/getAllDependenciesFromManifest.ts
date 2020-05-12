import { Dependencies, ProjectManifest } from '@pnpm/types'

export default function getAllDependenciesFromManifest (
  pkg: Pick<ProjectManifest, 'devDependencies' | 'dependencies' | 'optionalDependencies'>
): Dependencies {
  return {
    ...pkg.devDependencies,
    ...pkg.dependencies,
    ...pkg.optionalDependencies,
  } as Dependencies
}
