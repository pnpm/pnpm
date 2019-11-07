import getAllDependenciesFromPackage from './getAllDependenciesFromPackage'
import normalizeRegistries, { DEFAULT_REGISTRIES } from './normalizeRegistries'
import pickRegistryForPackage from './pickRegistryForPackage'
import realNodeModulesDir from './realNodeModulesDir'
import safeReadPackage, { fromDir as safeReadPackageFromDir } from './safeReadPkg'

export const readPackage = safeReadPackage

export {
  DEFAULT_REGISTRIES,
  getAllDependenciesFromPackage,
  normalizeRegistries,
  pickRegistryForPackage,
  realNodeModulesDir,
  safeReadPackage,
  safeReadPackageFromDir,
}

export * from './nodeIdUtils'
