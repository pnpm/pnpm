import getAllDependenciesFromPackage from './getAllDependenciesFromPackage'
import normalizeRegistries, { DEFAULT_REGISTRIES } from './normalizeRegistries'
import parseWantedDependency from './parseWantedDependency'
import pickRegistryForPackage from './pickRegistryForPackage'
import realNodeModulesDir from './realNodeModulesDir'
import safeReadPackage, { fromDir as safeReadPackageFromDir } from './safeReadPkg'

export const readPackage = safeReadPackage

export {
  DEFAULT_REGISTRIES,
  getAllDependenciesFromPackage,
  normalizeRegistries,
  parseWantedDependency,
  pickRegistryForPackage,
  realNodeModulesDir,
  safeReadPackage,
  safeReadPackageFromDir,
}

export * from './filterDependenciesByType'
export * from './nodeIdUtils'
