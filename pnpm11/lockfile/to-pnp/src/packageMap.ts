import { promises as fs } from 'node:fs'
import path from 'node:path'

import { depPathToFilename, refToRelative } from '@pnpm/deps.path'
import type { LockfileObject, PackageSnapshot, ProjectSnapshot } from '@pnpm/lockfile.fs'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { logger } from '@pnpm/logger'
import type { DepPath } from '@pnpm/types'

import { dependenciesGraphToPackageMap } from './dependenciesGraphPackageMap.js'
import { joinPath, type LinkBase, resolveLinkTarget, resolvePath, sortedEntries } from './packageMapPaths.js'
import {
  addDependencyLocation,
  addExternalLinkPackage,
  addPackage,
  addPackageLocation,
  buildPackageMap,
  createPackageMapState,
  type PackageMapState,
} from './packageMapState.js'
import type { DependenciesGraphPackageMapOptions, PackageMap, PackageMapOptions } from './packageMapTypes.js'

export { dependenciesGraphToPackageMap } from './dependenciesGraphPackageMap.js'
export type {
  DependenciesGraphPackageMapOptions,
  PackageMap,
  PackageMapGraphNode,
  PackageMapOptions,
  PackageMapPackage,
  PackageMapType,
} from './packageMapTypes.js'

export const PACKAGE_MAP_FILENAME = '.package-map.json'

/**
 * Delete a `.package-map.json` an earlier install wrote.
 *
 * An install that does not write the map must not leave the previous one
 * behind: the map is handed to Node by existence, so one left over from a run
 * with `nodeExperimentalPackageMap` on would be used again the moment the
 * setting came back on, describing a dependency set that has since changed.
 *
 * Best-effort and never rejects: an absent map is the wanted state, and any
 * other failure is logged at debug level rather than failing an install over a
 * file nothing is going to read. `remove_package_map` in pacquet makes the
 * same promise and logs the same way, so both stacks leave an install in the
 * same state, and leave the same trace, when the removal cannot happen.
 */
export async function removePackageMap (rootModulesDir: string): Promise<void> {
  const packageMapPath = path.join(rootModulesDir, PACKAGE_MAP_FILENAME)
  try {
    await fs.rm(packageMapPath, { force: true })
  } catch (error: unknown) {
    logger.debug({ msg: `Failed to remove ${packageMapPath}`, error })
  }
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

interface LockfilePackageMapContext {
  lockfile: LockfileObject
  opts: PackageMapOptions
  state: PackageMapState
}

interface DependencyGroups {
  base: LinkBase
  groups: Array<Record<string, string> | undefined>
}

export function lockfileToPackageMap (
  lockfile: LockfileObject,
  opts: PackageMapOptions
): PackageMap {
  const ctx: LockfilePackageMapContext = { lockfile, opts, state: createPackageMapState(opts) }
  for (const [importerId, importer] of sortedEntries(Object.entries(lockfile.importers))) {
    addImporter(ctx, importerId, importer)
  }
  for (const [depPath, pkgSnapshot] of sortedEntries(Object.entries(lockfile.packages ?? {}))) {
    addLockfilePackage(ctx, depPath, pkgSnapshot)
  }
  return buildPackageMap(ctx.state)
}

function addImporter (ctx: LockfilePackageMapContext, importerId: string, importer: ProjectSnapshot): void {
  const { opts, state } = ctx
  const dependencyGroups: DependencyGroups = {
    base: { importerId },
    groups: [importer.dependencies, importer.optionalDependencies, importer.devDependencies],
  }
  const dependencies = new Map<string, string>()
  const importerName = opts.importerNames[importerId]
  if (importerName) {
    dependencies.set(importerName, importerId)
  }
  addDependencies(ctx, dependencies, dependencyGroups)
  addPackage(state, { id: importerId, dir: resolvePath(opts.lockfileDir, importerId), dependencies })
  if (state.isLoose) {
    addPhysicalDependencyLocations(ctx, resolvePath(opts.lockfileDir, importerId, 'node_modules'), dependencyGroups)
  }
}

function addLockfilePackage (ctx: LockfilePackageMapContext, depPath: string, pkgSnapshot: PackageSnapshot): void {
  const { opts, state } = ctx
  const { name } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
  const packageDir = opts.locationByDepPath?.[depPath] ?? joinPath(
    opts.virtualStoreDir,
    depPathToFilename(depPath as DepPath, opts.virtualStoreDirMaxLength),
    'node_modules',
    name
  )
  const dependencyGroups: DependencyGroups = {
    base: { packageDir },
    groups: [pkgSnapshot.dependencies, pkgSnapshot.optionalDependencies],
  }
  const dependencies = new Map<string, string>([[name, depPath]])
  addDependencies(ctx, dependencies, dependencyGroups)
  addPackage(state, { id: depPath, dir: packageDir, dependencies })
  if (state.isLoose) {
    addPackageLocation(state, packageDir, { name, id: depPath })
    addPhysicalDependencyLocations(ctx, joinPath(packageDir, 'node_modules'), dependencyGroups)
  }
}

function addDependencies (
  ctx: LockfilePackageMapContext,
  dependencies: Map<string, string>,
  dependencyGroups: DependencyGroups
): void {
  for (const [alias, ref] of dependencyGroups.groups.flatMap((deps) => sortedEntries(Object.entries(deps ?? {})))) {
    const dependencyId = resolveDependencyId(ctx, { alias, ref }, dependencyGroups.base)
    if (dependencyId == null) continue
    dependencies.set(alias, dependencyId)
  }
}

function addPhysicalDependencyLocations (
  ctx: LockfilePackageMapContext,
  modulesDir: string,
  dependencyGroups: DependencyGroups
): void {
  for (const [alias, ref] of dependencyGroups.groups.flatMap((deps) => Object.entries(deps ?? {}))) {
    const dependencyId = resolveDependencyId(ctx, { alias, ref }, dependencyGroups.base)
    if (dependencyId == null) continue
    addDependencyLocation(ctx.state, modulesDir, { name: alias, id: dependencyId })
  }
}

function resolveDependencyId (
  ctx: LockfilePackageMapContext,
  dependency: { alias: string, ref: string },
  base: LinkBase
): string | undefined {
  const { alias, ref } = dependency
  if (ref.startsWith('link:')) {
    const target = resolveLinkTarget(ctx.opts.lockfileDir, base, ref)
    addExternalLinkPackage(ctx.state, target)
    return target.id
  }
  const relDepPath = refToRelative(ref, alias)
  if (relDepPath == null || ctx.lockfile.packages?.[relDepPath] == null) return undefined
  return relDepPath
}
