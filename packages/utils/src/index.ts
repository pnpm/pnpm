import getAllDependenciesFromPackage from './getAllDependenciesFromPackage'
import getSaveType from './getSaveType'
import realNodeModulesDir from './realNodeModulesDir'
import safeReadPackage, { fromDir as safeReadPackageFromDir } from './safeReadPkg'

export const readPackage = safeReadPackage

export {
  getAllDependenciesFromPackage,
  getSaveType,
  realNodeModulesDir,
  safeReadPackage,
  safeReadPackageFromDir,
}

export * from './nodeIdUtils'
export * from './getWantedDependencies'
