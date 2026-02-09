import { getAllUniqueSpecs } from './getAllUniqueSpecs.js'
import { getSpecFromPackageManifest } from './getSpecFromPackageManifest.js'

export * from './convertEnginesRuntimeToDependencies.js'
export * from './updateProjectManifestObject.js'
export * from './getDependencyTypeFromManifest.js'

export { getSpecFromPackageManifest, getAllUniqueSpecs }

export { filterDependenciesByType } from './filterDependenciesByType.js'
export { getAllDependenciesFromManifest } from './getAllDependenciesFromManifest.js'
