import { promises as fs } from 'node:fs'
import path from 'node:path'
import { pathToFileURL } from 'node:url'

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
  /**
   * Real on-disk directory of each package, keyed by depPath. Required for the
   * global virtual store, where packages live at a content-hashed path that
   * cannot be derived from the depPath alone. Falls back to the local
   * `<virtualStoreDir>/<depPathToFilename>` layout when a depPath is absent.
   */
  locationByDepPath?: Record<string, string>
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
  // Serialized compact, not pretty-printed: it lives in `node_modules` and is
  // read by tooling, not humans, so the formatting only adds CPU and bytes on
  // the install path.
  await fs.writeFile(
    path.join(opts.rootModulesDir, PACKAGE_MAP_FILENAME),
    `${JSON.stringify(lockfileToPackageMap(lockfile, opts))}\n`,
    'utf8'
  )
}

export async function writePackageMapFromDependenciesGraph (
  opts: DependenciesGraphPackageMapOptions
): Promise<void> {
  await fs.mkdir(opts.rootModulesDir, { recursive: true })
  // Compact serialization, like `writePackageMap` above.
  await fs.writeFile(
    path.join(opts.rootModulesDir, PACKAGE_MAP_FILENAME),
    `${JSON.stringify(dependenciesGraphToPackageMap(opts))}\n`,
    'utf8'
  )
}

export function lockfileToPackageMap (
  lockfile: LockfileObject,
  opts: PackageMapOptions
): PackageMap {
  const isLoose = opts.packageMapType === 'loose'
  // Keyed by filesystem-derived IDs (importer ids, `link:` targets), so a
  // dependency or project literally named `__proto__` must not reach the
  // object prototype. A null-prototype map keeps those keys as plain entries.
  const packages: PackageMap['packages'] = Object.create(null)
  const packageLocationsByModulesDir = isLoose ? new Map<string, Map<string, string>>() : undefined
  const packageDirsById = isLoose ? new Map<string, string>() : undefined

  const addPackage = (id: string, packageDir: string, dependencies: Map<string, string>) => {
    packageDirsById?.set(id, packageDir)
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
    if (packageLocationsByModulesDir == null) return
    const modulesDir = getNodeModulesPath(packageLocation)
    if (modulesDir == null) return
    addPackageToModulesDir(packageLocationsByModulesDir, modulesDir, packageName, packageId)
  }

  const addDependencyLocation = (modulesDir: string, dependencyName: string, dependencyId: string) => {
    if (packageLocationsByModulesDir == null) return
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
    addPackage(importerId, resolvePath(opts.lockfileDir, importerId), dependencies)
    if (isLoose) {
      const importerModulesDir = resolvePath(opts.lockfileDir, importerId, 'node_modules')
      addPhysicalDependencyLocations(importerModulesDir, importer.dependencies, { importerId })
      addPhysicalDependencyLocations(importerModulesDir, importer.optionalDependencies, { importerId })
      addPhysicalDependencyLocations(importerModulesDir, importer.devDependencies, { importerId })
    }
  }

  for (const [depPath, pkgSnapshot] of Object.entries(lockfile.packages ?? {}).sort(([a], [b]) => compareStrings(a, b))) {
    const { name } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
    const packageDir = opts.locationByDepPath?.[depPath] ?? joinPath(
      opts.virtualStoreDir,
      depPathToFilename(depPath as DepPath, opts.virtualStoreDirMaxLength),
      'node_modules',
      name
    )
    const dependencies = new Map<string, string>([[name, depPath]])
    addDependencies(dependencies, pkgSnapshot.dependencies)
    addDependencies(dependencies, pkgSnapshot.optionalDependencies)
    addPackage(depPath, packageDir, dependencies)
    if (isLoose) {
      addPackageLocation(name, packageDir, depPath)
      const packageModulesDir = joinPath(packageDir, 'node_modules')
      addPhysicalDependencyLocations(packageModulesDir, pkgSnapshot.dependencies)
      addPhysicalDependencyLocations(packageModulesDir, pkgSnapshot.optionalDependencies)
    }
  }

  if (isLoose) {
    for (const [id, packageDir] of packageDirsById!) {
      packages[id].dependencies = serializeDependencies(new Map([
        ...Object.entries(packages[id].dependencies),
        ...physicalDependencies(packageDir, packageLocationsByModulesDir!),
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
    deps: Record<string, string> | undefined,
    physicalOpts?: { importerId: string }
  ) {
    for (const [alias, ref] of Object.entries(deps ?? {})) {
      if (ref.startsWith('link:')) {
        const target = resolveLinkTarget(opts.lockfileDir, physicalOpts?.importerId, ref)
        addExternalLinkPackage(target)
        addDependencyLocation(modulesDir, alias, target.id)
        continue
      }
      const relDepPath = refToRelative(ref, alias)
      if (relDepPath == null || lockfile.packages?.[relDepPath] == null) continue
      addDependencyLocation(modulesDir, alias, relDepPath)
    }
  }
}

export function dependenciesGraphToPackageMap (
  opts: DependenciesGraphPackageMapOptions
): PackageMap {
  const isLoose = opts.packageMapType === 'loose'
  // See `lockfileToPackageMap`: null-prototype guard against `__proto__` ids.
  const packages: PackageMap['packages'] = Object.create(null)
  const packageIdsByGraphKey = new Map<string, string>()
  const packageDirsById = isLoose ? new Map<string, string>() : undefined
  const packageLocationsByModulesDir = isLoose ? new Map<string, Map<string, string>>() : undefined

  const addPackage = (id: string, packageDir: string, dependencies: Map<string, string>) => {
    packageDirsById?.set(id, packageDir)
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

  const addDependencyLocation = (modulesDir: string, dependencyName: string, dependencyId: string) => {
    if (packageLocationsByModulesDir == null) return
    addPackageToModulesDir(packageLocationsByModulesDir, modulesDir, dependencyName, dependencyId)
  }

  for (const [graphKey, node] of Object.entries(opts.graph).sort(([a], [b]) => compareStrings(a, b))) {
    packageIdsByGraphKey.set(graphKey, graphNodePackageId(node, opts))
    const modulesDir = isLoose ? getNodeModulesPath(node.dir) : undefined
    if (modulesDir && packageLocationsByModulesDir != null) {
      addPackageToModulesDir(packageLocationsByModulesDir, modulesDir, node.name, graphNodePackageId(node, opts))
    }
  }

  for (const [importerId, importer] of Object.entries(opts.lockfile.importers).sort(([a], [b]) => compareStrings(a, b))) {
    const importerDir = resolvePath(opts.lockfileDir, importerId)
    const importerPackageId = graphPackageId(importerDir, opts)
    const dependencies = new Map<string, string>()
    const importerName = opts.importerNames[importerId]
    if (importerName) {
      dependencies.set(importerName, importerPackageId)
    }
    addDirectDependencies(dependencies, opts.directDependenciesByImporterId[importerId])
    const importerModulesDir = isLoose ? joinPath(importerDir, 'node_modules') : undefined
    addLinkedDependencies(dependencies, importer.dependencies, { importerId, modulesDir: importerModulesDir })
    addLinkedDependencies(dependencies, importer.optionalDependencies, { importerId, modulesDir: importerModulesDir })
    addLinkedDependencies(dependencies, importer.devDependencies, { importerId, modulesDir: importerModulesDir })
    addPackage(importerPackageId, importerDir, dependencies)
  }

  for (const [graphKey, node] of Object.entries(opts.graph).sort(([a], [b]) => compareStrings(a, b))) {
    const dependencies = new Map<string, string>([[node.name, packageIdsByGraphKey.get(graphKey)!]])
    addGraphDependencies(dependencies, node.children)

    const pkgSnapshot = opts.lockfile.packages?.[node.depPath]
    if (pkgSnapshot) {
      const packageModulesDir = isLoose ? joinPath(node.dir, 'node_modules') : undefined
      addLinkedDependencies(dependencies, pkgSnapshot.dependencies, { modulesDir: packageModulesDir })
      addLinkedDependencies(dependencies, pkgSnapshot.optionalDependencies, { modulesDir: packageModulesDir })
    }

    addPackage(packageIdsByGraphKey.get(graphKey)!, node.dir, dependencies)
  }

  if (isLoose) {
    for (const [id, packageDir] of packageDirsById!) {
      packages[id].dependencies = serializeDependencies(new Map([
        ...Object.entries(packages[id].dependencies),
        ...physicalDependencies(packageDir, packageLocationsByModulesDir!),
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
    linkedOpts: {
      importerId?: string
      modulesDir?: string
    } = {}
  ) {
    for (const [alias, ref] of Object.entries(deps ?? {}).sort(([a], [b]) => compareStrings(a, b))) {
      if (!ref.startsWith('link:')) continue
      const target = resolveLinkTarget(opts.lockfileDir, linkedOpts.importerId, ref)
      const targetId = opts.packageIdStrategy === 'path'
        ? graphPackageId(target.dir, opts)
        : target.id
      addExternalLinkPackage({
        ...target,
        id: targetId,
      })
      dependencies.set(alias, targetId)
      if (linkedOpts.modulesDir) {
        addDependencyLocation(linkedOpts.modulesDir, alias, targetId)
      }
    }
  }
}

interface LinkTarget {
  id: string
  dir: string
}

function resolveLinkTarget (lockfileDir: string, importerId: string | undefined, ref: string): LinkTarget {
  const linkPath = ref.slice(5)
  // Detect the path flavor from `linkPath`, not the raw `ref`: the `link:`
  // prefix would hide a Windows-absolute target (e.g. `link:C:\x`) from the
  // drive-letter check, making it look relative on POSIX.
  const pathUtils = getPathUtils(lockfileDir, linkPath)
  const importerDir = pathUtils.resolve(lockfileDir, importerId ?? '.')
  const dir = pathUtils.isAbsolute(linkPath)
    ? linkPath
    : pathUtils.resolve(importerDir, linkPath)
  const relativeId = relativePath(lockfileDir, dir)
  return {
    id: relativeId == null || relativeId.startsWith('..') ? `link:${normalizePath(dir)}` : relativeId,
    dir,
  }
}

function toRelativeUrl (from: string, to: string): string {
  // No meaningful relative path exists between a POSIX dir and a
  // Windows-absolute target (or vice versa), so emit an absolute file URL
  // rather than letting `path.win32.relative` produce a bogus relative string.
  const toIsWindows = isWindowsAbsolutePath(to)
  if (toIsWindows !== isWindowsAbsolutePath(from)) {
    return pathToFileURL(to, { windows: toIsWindows }).href
  }
  const pathUtils = getPathUtils(from, to)
  const relative = pathUtils.relative(from, to)
  if (pathUtils.isAbsolute(relative)) {
    return pathToFileURL(to, { windows: pathUtils === path.win32 }).href
  }
  const normalizedRelativePath = normalizePath(relative) || '.'
  if (normalizedRelativePath === '.' || normalizedRelativePath === '..' || normalizedRelativePath.startsWith('./') || normalizedRelativePath.startsWith('../')) {
    return normalizedRelativePath
  }
  return `./${normalizedRelativePath}`
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
  const pathUtils = getPathUtils(packageDir)
  let currentPath = packageDir
  while (true) {
    const modulesDir = normalizePath(pathUtils.join(currentPath, 'node_modules'))
    const packageLocations = packageLocationsByModulesDir.get(modulesDir)
    if (packageLocations) {
      for (const [dependencyName, packageId] of Array.from(packageLocations).sort(([a], [b]) => compareStrings(a, b))) {
        if (!dependencies.has(dependencyName)) {
          dependencies.set(dependencyName, packageId)
        }
      }
    }

    const parentPath = pathUtils.dirname(currentPath)
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
  const relativeId = relativePath(opts.rootModulesDir, packageDir)
  if (relativeId == null) return `link:${normalizePath(packageDir)}`
  return relativeId === '..' ? '.' : relativeId
}

type PathUtils = typeof path.posix

const WINDOWS_ABSOLUTE_PATH_REGEXP = /^(?:[a-z]:[\\/]|[/\\]{2}[^/\\])/i

function resolvePath (from: string, ...segments: string[]): string {
  return getPathUtils(from, ...segments).resolve(from, ...segments)
}

function joinPath (from: string, ...segments: string[]): string {
  return getPathUtils(from, ...segments).join(from, ...segments)
}

function relativePath (from: string, to: string): string | undefined {
  const pathUtils = getPathUtils(from, to)
  const relative = pathUtils.relative(from, to)
  if (pathUtils.isAbsolute(relative)) return undefined
  return normalizePath(relative) || '.'
}

function getPathUtils (...paths: string[]): PathUtils {
  return paths.some(isWindowsAbsolutePath) ? path.win32 : path
}

function isWindowsAbsolutePath (pathLike: string): boolean {
  return WINDOWS_ABSOLUTE_PATH_REGEXP.test(pathLike)
}

function compareStrings (a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0
}
