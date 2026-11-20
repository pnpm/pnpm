import normalizePath from 'normalize-path'

import {
  getNodeModulesPath,
  getPathUtils,
  type LinkTarget,
  sortedEntries,
  toRelativeUrl,
} from './packageMapPaths.js'
import type { PackageMap, PackageMapType } from './packageMapTypes.js'

/**
 * The package map under construction. In loose mode it also records where each
 * package sits on disk, so that every package can additionally resolve the
 * packages Node would find by walking up its `node_modules` directories.
 */
export interface PackageMapState {
  isLoose: boolean
  packages: PackageMap['packages']
  packageDirsById?: Map<string, string>
  packageLocationsByModulesDir?: Map<string, Map<string, string>>
  rootModulesDir: string
}

export interface PackageMapEntry {
  id: string
  dir: string
  dependencies: Map<string, string>
}

export interface ModulesDirEntry {
  name: string
  id: string
}

export function createPackageMapState (opts: { packageMapType?: PackageMapType, rootModulesDir: string }): PackageMapState {
  const isLoose = opts.packageMapType === 'loose'
  return {
    isLoose,
    // Keyed by filesystem-derived IDs (importer ids, `link:` targets), so a
    // dependency or project literally named `__proto__` must not reach the
    // object prototype. A null-prototype map keeps those keys as plain entries.
    packages: Object.create(null),
    packageDirsById: isLoose ? new Map<string, string>() : undefined,
    packageLocationsByModulesDir: isLoose ? new Map<string, Map<string, string>>() : undefined,
    rootModulesDir: opts.rootModulesDir,
  }
}

export function addPackage (state: PackageMapState, entry: PackageMapEntry): void {
  state.packageDirsById?.set(entry.id, entry.dir)
  state.packages[entry.id] = {
    url: toRelativeUrl(state.rootModulesDir, entry.dir),
    dependencies: serializeDependencies(entry.dependencies),
  }
}

export function addExternalLinkPackage (state: PackageMapState, target: LinkTarget): void {
  state.packages[target.id] ??= {
    url: toRelativeUrl(state.rootModulesDir, target.dir),
    dependencies: {},
  }
}

export function addPackageLocation (state: PackageMapState, packageLocation: string, entry: ModulesDirEntry): void {
  if (state.packageLocationsByModulesDir == null) return
  const modulesDir = getNodeModulesPath(packageLocation)
  if (modulesDir == null) return
  addPackageToModulesDir(state.packageLocationsByModulesDir, modulesDir, entry)
}

export function addDependencyLocation (state: PackageMapState, modulesDir: string, entry: ModulesDirEntry): void {
  if (state.packageLocationsByModulesDir == null) return
  addPackageToModulesDir(state.packageLocationsByModulesDir, modulesDir, entry)
}

export function buildPackageMap (state: PackageMapState): PackageMap {
  const { packages, packageDirsById, packageLocationsByModulesDir } = state
  if (packageDirsById != null && packageLocationsByModulesDir != null) {
    for (const [id, packageDir] of packageDirsById) {
      packages[id].dependencies = serializeDependencies(new Map([
        ...Object.entries(packages[id].dependencies),
        ...physicalDependencies(packageDir, packageLocationsByModulesDir),
      ]))
    }
  }
  return {
    packages: Object.fromEntries(sortedEntries(Object.entries(packages))),
  }
}

function addPackageToModulesDir (
  packageLocationsByModulesDir: Map<string, Map<string, string>>,
  modulesDir: string,
  entry: ModulesDirEntry
) {
  const normalizedModulesDir = normalizePath(modulesDir)
  let packageLocations = packageLocationsByModulesDir.get(normalizedModulesDir)
  if (packageLocations == null) {
    packageLocations = new Map()
    packageLocationsByModulesDir.set(normalizedModulesDir, packageLocations)
  }
  packageLocations.set(entry.name, entry.id)
}

function physicalDependencies (
  packageDir: string,
  packageLocationsByModulesDir: Map<string, Map<string, string>>
): Map<string, string> {
  const dependencies = new Map<string, string>()
  const pathUtils = getPathUtils(packageDir)
  let currentPath = packageDir
  while (true) {
    const modulesDir = normalizePath(pathUtils.join(currentPath, 'node_modules'))
    const packageLocations = packageLocationsByModulesDir.get(modulesDir)
    if (packageLocations) {
      addMissingDependencies(dependencies, packageLocations)
    }

    const parentPath = pathUtils.dirname(currentPath)
    if (parentPath === currentPath) break
    currentPath = parentPath
  }
  return dependencies
}

function addMissingDependencies (dependencies: Map<string, string>, packageLocations: Map<string, string>): void {
  for (const [dependencyName, packageId] of sortedEntries(packageLocations)) {
    if (!dependencies.has(dependencyName)) {
      dependencies.set(dependencyName, packageId)
    }
  }
}

function serializeDependencies (dependencies: Map<string, string>): Record<string, string> {
  return Object.fromEntries(sortedEntries(dependencies))
}
