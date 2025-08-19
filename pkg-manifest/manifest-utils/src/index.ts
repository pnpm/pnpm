import {
  type Dependencies,
  type IncludedDependencies,
  type ProjectManifest,
} from '@pnpm/types'
import { getAllUniqueSpecs } from './getAllUniqueSpecs.js'
import { getSpecFromPackageManifest } from './getSpecFromPackageManifest.js'

export * from './updateProjectManifestObject.js'
export * from './getDependencyTypeFromManifest.js'

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
  }
}
