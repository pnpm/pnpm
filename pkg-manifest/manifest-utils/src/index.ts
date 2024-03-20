import '@total-typescript/ts-reset'
import type {
  Dependencies,
  ProjectManifest,
  IncludedDependencies,
} from '@pnpm/types'

import { getSpecFromPackageManifest } from './getSpecFromPackageManifest'

export * from './getPref'
export * from './updateProjectManifestObject'
export * from './getDependencyTypeFromManifest'

export { getSpecFromPackageManifest }

export function filterDependenciesByType(
  manifest: ProjectManifest | undefined,
  include: IncludedDependencies
): Dependencies {
  return {
    ...(include.devDependencies ? manifest?.devDependencies : {}),
    ...(include.dependencies ? manifest?.dependencies : {}),
    ...(include.optionalDependencies ? manifest?.optionalDependencies : {}),
  }
}

export function getAllDependenciesFromManifest(
  manifest: Pick<
    ProjectManifest,
    'devDependencies' | 'dependencies' | 'optionalDependencies'
  > | undefined
): Dependencies {
  return {
    ...manifest?.devDependencies,
    ...manifest?.dependencies,
    ...manifest?.optionalDependencies,
  }
}
