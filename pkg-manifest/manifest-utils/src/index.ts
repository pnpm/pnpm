import {
  type Dependencies,
  type IncludedDependencies,
  type ProjectManifest,
} from '@pnpm/types'
import { getAllUniqueSpecs } from './getAllUniqueSpecs'
import { getSpecFromPackageManifest } from './getSpecFromPackageManifest'

export * from './getPref'
export * from './updateProjectManifestObject'
export * from './getDependencyTypeFromManifest'

export { getSpecFromPackageManifest, getAllUniqueSpecs }

export function filterDependenciesByType (
  manifest: ProjectManifest,
  include: IncludedDependencies
): Dependencies {
  return {
    ...(include.devDependencies ? manifest.devDependencies : {}),
    ...(include.dependencies ? manifest.dependencies : {}),
    ...(include.optionalDependencies ? manifest.optionalDependencies : {}),
  }
}

export function getAllDependenciesFromManifest (
  manifest: Pick<ProjectManifest, 'devDependencies' | 'dependencies' | 'optionalDependencies'>
): Dependencies {
  return {
    ...manifest.devDependencies,
    ...manifest.dependencies,
    ...manifest.optionalDependencies,
  } as Dependencies
}
