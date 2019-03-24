import getAllDependenciesFromPackage from './getAllDependenciesFromPackage'
import getSaveType from './getSaveType'
import normalizeRegistries, { DEFAULT_REGISTRIES } from './normalizeRegistries'
import pickRegistryForPackage from './pickRegistryForPackage'
import realNodeModulesDir from './realNodeModulesDir'
import safeReadPackage, { fromDir as safeReadPackageFromDir } from './safeReadPkg'

export const readPackage = safeReadPackage

export {
  DEFAULT_REGISTRIES,
  getAllDependenciesFromPackage,
  getSaveType,
  normalizeRegistries,
  pickRegistryForPackage,
  realNodeModulesDir,
  safeReadPackage,
  safeReadPackageFromDir,
}

export * from './nodeIdUtils'
export * from './getWantedDependencies'
