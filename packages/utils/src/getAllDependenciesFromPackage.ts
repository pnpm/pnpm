import { Dependencies, ImporterManifest } from '@pnpm/types'

export default function getAllDependenciesFromPackage (
  pkg: Pick<ImporterManifest, 'devDependencies' | 'dependencies' | 'optionalDependencies'>,
): Dependencies {
  return {
    ...pkg.devDependencies,
    ...pkg.dependencies,
    ...pkg.optionalDependencies,
  } as Dependencies
}
