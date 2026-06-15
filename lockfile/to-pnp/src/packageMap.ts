import { promises as fs } from 'node:fs'
import path from 'node:path'

import { depPathToFilename, refToRelative } from '@pnpm/deps.path'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import type { DepPath } from '@pnpm/types'
import normalizePath from 'normalize-path'

export const PACKAGE_MAP_FILENAME = '.package-map.json'

export interface PackageMap {
  packages: Record<string, PackageMapPackage>
}

export interface PackageMapPackage {
  url: string
  dependencies: Record<string, string>
}

export type PackageMapType = 'standard' | 'loose'

export interface PackageMapOptions {
  importerNames: Record<string, string | undefined>
  lockfileDir: string
  packageMapType?: PackageMapType
  rootModulesDir: string
  virtualStoreDir: string
  virtualStoreDirMaxLength: number
}

export interface DependenciesGraphPackageMapOptions {
  directDependenciesByImporterId: Record<string, Record<string, string>>
  graph: Record<string, PackageMapGraphNode>
  importerNames: Record<string, string | undefined>
  lockfile: LockfileObject
  lockfileDir: string
  packageMapType?: PackageMapType
  rootModulesDir: string
  packageIdStrategy: 'depPath' | 'path'
}

export interface PackageMapGraphNode {
  children: Record<string, string>
  depPath: DepPath
  dir: string
  name: string
}

export async function writePackageMap (
  lockfile: LockfileObject,
  opts: PackageMapOptions
): Promise<void> {
  await fs.mkdir(opts.rootModulesDir, { recursive: true })
  await fs.writeFile(
    path.join(opts.rootModulesDir, PACKAGE_MAP_FILENAME),
    `${JSON.stringify(lockfileToPackageMap(lockfile, opts), null, 2)}\n`,
    'utf8'
  )
}

export async function writePackageMapFromDependenciesGraph (
  opts: DependenciesGraphPackageMapOptions
): Promise<void> {
  await fs.mkdir(opts.rootModulesDir, { recursive: true })
  await fs.writeFile(
    path.join(opts.rootModulesDir, PACKAGE_MAP_FILENAME),
    `${JSON.stringify(dependenciesGraphToPackageMap(opts), null, 2)}\n`,
    'utf8'
  )
}

export function lockfileToPackageMap (
  lockfile: LockfileObject,
  opts: PackageMapOptions
): PackageMap {
  const packages: PackageMap['packages'] = {}
  const packageLocationsByModulesDir = new Map<string, Map<string, string>>()
  const packageDirsById = new Map<string, string>()

  const addPackage = (id: string, packageDir: string, dependencies: Map<string, string>) => {
    packageDirsById.set(id, packageDir)
    packages[id] = {
      url: toRelativeUrl(opts.rootModulesDir, packageDir),
      dependencies: Object.fromEntries(Array.from(dependencies).sort(([a], [b]) => compareStrings(a, b))),
    }
  }

  const addExternalLinkPackage = (target: LinkTarget) => {
    packages[target.id] ??= {
      url: toRelativeUrl(opts.rootModulesDir, target.dir),
      dependencies: {},
    }
  }

  const addPackageLocation = (packageName: string, packageLocation: string, packageId: string) => {
    const modulesDir = getNodeModulesPath(packageLocation)
    if (modulesDir == null) return
    addPackageToModulesDir(packageLocationsByModulesDir, modulesDir, packageName, packageId)
  }

  const addDependencyLocation = (modulesDir: string, dependencyName: string, dependencyId: string) => {
    addPackageToModulesDir(packageLocationsByModulesDir, modulesDir, dependencyName, dependencyId)
  }

  for (const [importerId, importer] of Object.entries(lockfile.importers).sort(([a], [b]) => compareStrings(a, b))) {
    const dependencies = new Map<string, string>()
    const importerName = opts.importerNames[importerId]
    if (importerName) {
      dependencies.set(importerName, importerId)
    }
    addDependencies(dependencies, importer.dependencies, { importerId })
    addDependencies(dependencies, importer.optionalDependencies, { importerId })
    addDependencies(dependencies, importer.devDependencies, { importerId })
    addPackage(importerId, path.resolve(opts.lockfileDir, importerId), dependencies)
    addPhysicalDependencyLocations(path.resolve(opts.lockfileDir, importerId, 'node_modules'), importer.dependencies)
    addPhysicalDependencyLocations(path.resolve(opts.lockfileDir, importerId, 'node_modules'), importer.optionalDependencies)
    addPhysicalDependencyLocations(path.resolve(opts.lockfileDir, importerId, 'node_modules'), importer.devDependencies)
  }

  for (const [depPath, pkgSnapshot] of Object.entries(lockfile.packages ?? {}).sort(([a], [b]) => compareStrings(a, b))) {
    const { name } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
    const packageDir = path.join(
      opts.virtualStoreDir,
      depPathToFilename(depPath as DepPath, opts.virtualStoreDirMaxLength),
      'node_modules',
      name
    )
    const dependencies = new Map<string, string>([[name, depPath]])
    addDependencies(dependencies, pkgSnapshot.dependencies)
    addDependencies(dependencies, pkgSnapshot.optionalDependencies)
    addPackage(depPath, packageDir, dependencies)
    addPackageLocation(name, packageDir, depPath)
    addPhysicalDependencyLocations(path.join(packageDir, 'node_modules'), pkgSnapshot.dependencies)
    addPhysicalDependencyLocations(path.join(packageDir, 'node_modules'), pkgSnapshot.optionalDependencies)
  }

  if (opts.packageMapType === 'loose') {
    for (const [id, packageDir] of packageDirsById) {
      packages[id].dependencies = serializeDependencies(new Map([
        ...Object.entries(packages[id].dependencies),
        ...physicalDependencies(packageDir, packageLocationsByModulesDir),
      ]))
    }
  }

  return {
    packages: Object.fromEntries(Object.entries(packages).sort(([a], [b]) => compareStrings(a, b))),
  }

  function addDependencies (
    dependencies: Map<string, string>,
    deps: Record<string, string> | undefined,
    opts?: { importerId: string }
  ) {
    for (const [alias, ref] of Object.entries(deps ?? {}).sort(([a], [b]) => compareStrings(a, b))) {
      const dependencyId = resolveDependencyId(alias, ref, opts)
      if (dependencyId == null) continue
      dependencies.set(alias, dependencyId)
    }
  }

  function resolveDependencyId (
    alias: string,
    ref: string,
    depOpts: { importerId: string } | undefined
  ): string | undefined {
    if (ref.startsWith('link:')) {
      const target = resolveLinkTarget(opts.lockfileDir, depOpts?.importerId, ref)
      addExternalLinkPackage(target)
      return target.id
    }
    const relDepPath = refToRelative(ref, alias)
    if (relDepPath == null || lockfile.packages?.[relDepPath] == null) return undefined
    return relDepPath
  }

  function addPhysicalDependencyLocations (
    modulesDir: string,
    deps: Record<string, string> | undefined
  ) {
    for (const [alias, ref] of Object.entries(deps ?? {})) {
      if (ref.startsWith('link:')) continue
      const relDepPath = refToRelative(ref, alias)
      if (relDepPath == null || lockfile.packages?.[relDepPath] == null) continue
      addDependencyLocation(modulesDir, alias, relDepPath)
    }
  }
}

export function dependenciesGraphToPackageMap (
  opts: DependenciesGraphPackageMapOptions
): PackageMap {
  const packages: PackageMap['packages'] = {}
  const packageIdsByGraphKey = new Map<string, string>()
  const packageDirsById = new Map<string, string>()
  const packageLocationsByModulesDir = new Map<string, Map<string, string>>()

  const addPackage = (id: string, packageDir: string, dependencies: Map<string, string>) => {
    packageDirsById.set(id, packageDir)
    packages[id] = {
      url: toRelativeUrl(opts.rootModulesDir, packageDir),
      dependencies: Object.fromEntries(Array.from(dependencies).sort(([a], [b]) => compareStrings(a, b))),
    }
  }

  const addExternalLinkPackage = (target: LinkTarget) => {
    packages[target.id] ??= {
      url: toRelativeUrl(opts.rootModulesDir, target.dir),
      dependencies: {},
    }
  }

  for (const [graphKey, node] of Object.entries(opts.graph).sort(([a], [b]) => compareStrings(a, b))) {
    packageIdsByGraphKey.set(graphKey, graphNodePackageId(node, opts))
    const modulesDir = getNodeModulesPath(node.dir)
    if (modulesDir) {
      addPackageToModulesDir(packageLocationsByModulesDir, modulesDir, node.name, graphNodePackageId(node, opts))
    }
  }

  for (const [importerId, importer] of Object.entries(opts.lockfile.importers).sort(([a], [b]) => compareStrings(a, b))) {
    const importerPackageId = graphPackageId(path.resolve(opts.lockfileDir, importerId), opts)
    const dependencies = new Map<string, string>()
    const importerName = opts.importerNames[importerId]
    if (importerName) {
      dependencies.set(importerName, importerPackageId)
    }
    addDirectDependencies(dependencies, opts.directDependenciesByImporterId[importerId])
    addLinkedDependencies(dependencies, importer.dependencies, importerId)
    addLinkedDependencies(dependencies, importer.optionalDependencies, importerId)
    addLinkedDependencies(dependencies, importer.devDependencies, importerId)
    addPackage(importerPackageId, path.resolve(opts.lockfileDir, importerId), dependencies)
  }

  for (const [graphKey, node] of Object.entries(opts.graph).sort(([a], [b]) => compareStrings(a, b))) {
    const dependencies = new Map<string, string>([[node.name, packageIdsByGraphKey.get(graphKey)!]])
    addGraphDependencies(dependencies, node.children)

    const pkgSnapshot = opts.lockfile.packages?.[node.depPath]
    if (pkgSnapshot) {
      addLinkedDependencies(dependencies, pkgSnapshot.dependencies)
      addLinkedDependencies(dependencies, pkgSnapshot.optionalDependencies)
    }

    addPackage(packageIdsByGraphKey.get(graphKey)!, node.dir, dependencies)
  }

  if (opts.packageMapType === 'loose') {
    for (const [id, packageDir] of packageDirsById) {
      packages[id].dependencies = serializeDependencies(new Map([
        ...Object.entries(packages[id].dependencies),
        ...physicalDependencies(packageDir, packageLocationsByModulesDir),
      ]))
    }
  }

  return {
    packages: Object.fromEntries(Object.entries(packages).sort(([a], [b]) => compareStrings(a, b))),
  }

  function addDirectDependencies (
    dependencies: Map<string, string>,
    deps: Record<string, string> | undefined
  ) {
    for (const [alias, graphKey] of Object.entries(deps ?? {}).sort(([a], [b]) => compareStrings(a, b))) {
      const packageId = packageIdsByGraphKey.get(graphKey)
      if (packageId) dependencies.set(alias, packageId)
    }
  }

  function addGraphDependencies (
    dependencies: Map<string, string>,
    deps: Record<string, string> | undefined
  ) {
    for (const [alias, graphKey] of Object.entries(deps ?? {}).sort(([a], [b]) => compareStrings(a, b))) {
      const packageId = packageIdsByGraphKey.get(graphKey)
      if (packageId) dependencies.set(alias, packageId)
    }
  }

  function addLinkedDependencies (
    dependencies: Map<string, string>,
    deps: Record<string, string> | undefined,
    importerId?: string
  ) {
    for (const [alias, ref] of Object.entries(deps ?? {}).sort(([a], [b]) => compareStrings(a, b))) {
      if (!ref.startsWith('link:')) continue
      const target = resolveLinkTarget(opts.lockfileDir, importerId, ref)
      const targetId = opts.packageIdStrategy === 'path'
        ? graphPackageId(target.dir, opts)
        : target.id
      addExternalLinkPackage({
        ...target,
        id: targetId,
      })
      dependencies.set(alias, targetId)
    }
  }
}

interface LinkTarget {
  id: string
  dir: string
}

function resolveLinkTarget (lockfileDir: string, importerId: string | undefined, ref: string): LinkTarget {
  const importerDir = path.resolve(lockfileDir, importerId ?? '.')
  const linkPath = ref.slice(5)
  const dir = path.isAbsolute(linkPath)
    ? linkPath
    : path.resolve(importerDir, linkPath)
  const relativeId = normalizePath(path.relative(lockfileDir, dir)) || '.'
  return {
    id: relativeId.startsWith('..') ? `link:${normalizePath(dir)}` : relativeId,
    dir,
  }
}

function toRelativeUrl (from: string, to: string): string {
  const relativePath = normalizePath(path.relative(from, to)) || '.'
  if (relativePath === '.' || relativePath === '..' || relativePath.startsWith('./') || relativePath.startsWith('../')) {
    return relativePath
  }
  return `./${relativePath}`
}

function getNodeModulesPath (packageLocation: string): string | undefined {
  const segments = normalizePath(packageLocation).split('/')
  const nodeModulesIndex = segments.lastIndexOf('node_modules')
  if (nodeModulesIndex === -1) return undefined
  return segments.slice(0, nodeModulesIndex + 1).join('/')
}

function addPackageToModulesDir (
  packageLocationsByModulesDir: Map<string, Map<string, string>>,
  modulesDir: string,
  packageName: string,
  packageId: string
) {
  const normalizedModulesDir = normalizePath(modulesDir)
  let packageLocations = packageLocationsByModulesDir.get(normalizedModulesDir)
  if (packageLocations == null) {
    packageLocations = new Map()
    packageLocationsByModulesDir.set(normalizedModulesDir, packageLocations)
  }
  packageLocations.set(packageName, packageId)
}

function physicalDependencies (
  packageDir: string,
  packageLocationsByModulesDir: Map<string, Map<string, string>>
): Map<string, string> {
  const dependencies = new Map<string, string>()
  let currentPath = packageDir
  while (true) {
    const modulesDir = normalizePath(path.join(currentPath, 'node_modules'))
    const packageLocations = packageLocationsByModulesDir.get(modulesDir)
    if (packageLocations) {
      for (const [dependencyName, packageId] of Array.from(packageLocations).sort(([a], [b]) => compareStrings(a, b))) {
        if (!dependencies.has(dependencyName)) {
          dependencies.set(dependencyName, packageId)
        }
      }
    }

    const parentPath = path.dirname(currentPath)
    if (parentPath === currentPath) break
    currentPath = parentPath
  }
  return dependencies
}

function serializeDependencies (dependencies: Map<string, string>): Record<string, string> {
  return Object.fromEntries(Array.from(dependencies).sort(([a], [b]) => compareStrings(a, b)))
}

function graphNodePackageId (node: PackageMapGraphNode, opts: DependenciesGraphPackageMapOptions): string {
  if (opts.packageIdStrategy === 'depPath') return node.depPath
  return graphPackageId(node.dir, opts)
}

function graphPackageId (packageDir: string, opts: Pick<DependenciesGraphPackageMapOptions, 'rootModulesDir'>): string {
  const relativePath = normalizePath(path.relative(opts.rootModulesDir, packageDir)) || '.'
  return relativePath === '..' ? '.' : relativePath
}

function compareStrings (a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0
}
