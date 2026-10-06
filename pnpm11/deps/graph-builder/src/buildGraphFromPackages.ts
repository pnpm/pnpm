import fs from 'node:fs'
import path from 'node:path'

import { pickRegistryContext } from '@pnpm/config.normalize-registries'
import { packageIsInstallable } from '@pnpm/config.package-is-installable'
import {
  progressLogger,
} from '@pnpm/core-loggers'
import * as dp from '@pnpm/deps.path'
import { safeJoinModulesDir } from '@pnpm/fs.symlink-dependency'
import type { LockfileObject, LockfileResolution } from '@pnpm/lockfile.fs'
import {
  packageIdFromSnapshot,
  pkgSnapshotToResolution,
} from '@pnpm/lockfile.utils'
import { logger } from '@pnpm/logger'
import { getPatchInfo } from '@pnpm/patching.config'
import type { PatchInfo } from '@pnpm/patching.types'
import type { FetchResponse } from '@pnpm/store.controller-types'
import { pathExists } from 'path-exists'
import { equals, isEmpty } from 'ramda'

import { iteratePkgsForVirtualStore, type PkgSnapshotWithLocation } from './iteratePkgsForVirtualStore.js'
import type { DependenciesGraph, DependenciesGraphNode, LockfileToDepGraphOptions } from './lockfileToDepGraph.js'

const brokenModulesLogger = logger('_broken_node_modules')

type PkgMeta = PkgSnapshotWithLocation['pkgMeta']
type PackageSnapshot = PkgMeta['pkgSnapshot']

export interface GraphFromPackages {
  graph: DependenciesGraph
  locationByDepPath: Record<string, string>
  injectionTargetsByDepPath: Map<string, string[]>
}

interface BuildGraphContext extends GraphFromPackages {
  opts: LockfileToDepGraphOptions
  currentPackages: NonNullable<LockfileObject['packages']>
  getPatchInfo: (pkgName: string, pkgVersion: string) => PatchInfo | undefined
}

interface LocatedPkg {
  pkgMeta: PkgMeta
  packageId: string
  isDirectoryDep: boolean
  modules: string
  dir: string
}

interface PkgDirCheck {
  dir: string
  currentPkgSnapshot?: PackageSnapshot
  pkgSnapshot: PackageSnapshot
  isDirectoryDep: boolean
  isUnchanged: boolean
  mightNeedBuild: boolean
}

type PkgDirReuse = 'up-to-date' | 'reusable' | 'needs-fetch'

export async function buildGraphFromPackages (
  lockfile: LockfileObject,
  currentLockfile: LockfileObject | null,
  opts: LockfileToDepGraphOptions
): Promise<GraphFromPackages> {
  const ctx: BuildGraphContext = {
    opts,
    currentPackages: currentLockfile?.packages ?? {},
    graph: {},
    locationByDepPath: {},
    // Only populated for directory deps (injected workspace packages)
    injectionTargetsByDepPath: new Map<string, string[]>(),
    getPatchInfo: getPatchInfo.bind(null, opts.patchedDependencies),
  }
  const promises: Array<Promise<void>> = []
  for (const pkgLocation of iteratePkgsForVirtualStore(lockfile, opts)) {
    promises.push(addPkgToGraph(ctx, pkgLocation))
  }
  await Promise.all(promises)
  const { graph, locationByDepPath, injectionTargetsByDepPath } = ctx
  return { graph, locationByDepPath, injectionTargetsByDepPath }
}

async function addPkgToGraph (ctx: BuildGraphContext, { dirInVirtualStore, pkgMeta }: PkgSnapshotWithLocation): Promise<void> {
  const { opts } = ctx
  const { depPath, pkgSnapshot } = pkgMeta
  if (opts.skipped.has(depPath)) return

  const packageId = packageIdFromSnapshot(depPath, pkgSnapshot)
  if (!opts.includeIncompatiblePackages && isPkgIncompatible(opts, pkgMeta, packageId)) {
    opts.skipped.add(depPath)
    return
  }

  const isDirectoryDep = 'directory' in pkgSnapshot.resolution && pkgSnapshot.resolution.directory != null
  if (isDirectoryDep && opts.ignoreLocalPackages) {
    logger.info({
      message: `Skipping local dependency ${pkgMeta.name}@${pkgMeta.version} (file: protocol)`,
      prefix: opts.lockfileDir,
    })
    return
  }

  const modules = path.join(dirInVirtualStore, 'node_modules')
  // `pkgName` is reconstructed from the (attacker-controllable) lockfile
  // depPath key via `dp.parse`, which does no validation. Contain it here so
  // a traversal name (e.g. `../../../tmp/x`) can't make the package import
  // escape the virtual store. Mirrors the guard on the hoisted linker.
  const dir = safeJoinModulesDir(modules, pkgMeta.name)
  ctx.locationByDepPath[depPath] = dir
  // Track directory deps for injected workspace packages
  if (isDirectoryDep) {
    ctx.injectionTargetsByDepPath.set(depPath, [dir])
  }
  await addLocatedPkgToGraph(ctx, { pkgMeta, packageId, isDirectoryDep, modules, dir })
}

function isPkgIncompatible (opts: LockfileToDepGraphOptions, pkgMeta: PkgMeta, packageId: string): boolean {
  const { depPath, pkgSnapshot } = pkgMeta
  const pkg = {
    name: pkgMeta.name,
    version: pkgMeta.version,
    engines: opts.engineStrict && dp.hasPatchHash(depPath) ? undefined : pkgSnapshot.engines,
    cpu: pkgSnapshot.cpu,
    os: pkgSnapshot.os,
    libc: pkgSnapshot.libc,
  }
  return packageIsInstallable(packageId, pkg, {
    // An incompatibility inside an `optionalDependencies` subtree is
    // reported, not fatal — see `filterLockfileByImportersAndEngine`,
    // which classifies these dep paths.
    engineStrict: opts.engineStrict && pkgSnapshot.optional !== true,
    lockfileDir: opts.lockfileDir,
    nodeVersion: opts.nodeVersion,
    optional: !opts.requiredDepPaths.has(depPath),
    supportedArchitectures: opts.supportedArchitectures,
  }) === false
}

async function addLocatedPkgToGraph (ctx: BuildGraphContext, pkg: LocatedPkg): Promise<void> {
  const { opts } = ctx
  const { depPath, pkgSnapshot } = pkg.pkgMeta
  const currentPkgSnapshot = ctx.currentPackages[depPath]
  const depIntegrityIsUnchanged = isIntegrityEqual(pkgSnapshot.resolution, currentPkgSnapshot?.resolution)
  const dirReuse = await checkPkgDirReuse(opts, {
    dir: pkg.dir,
    currentPkgSnapshot,
    pkgSnapshot,
    isDirectoryDep: pkg.isDirectoryDep,
    isUnchanged: !pkg.isDirectoryDep &&
      currentPkgSnapshot != null &&
      equals(currentPkgSnapshot.dependencies, pkgSnapshot.dependencies) &&
      depIntegrityIsUnchanged,
    // In GVS mode, packages that are allowed to build may have a .pnpm-needs-build
    // marker indicating a previous build failed or was interrupted. When the
    // marker is present, skip the fast path to force a re-fetch/re-import/re-build.
    mightNeedBuild: opts.enableGlobalVirtualStore === true &&
      opts.allowBuild?.(depPath) === true,
  })
  if (dirReuse === 'up-to-date') return

  const fetchResponse = dirReuse === 'reusable' ? {} : await fetchPkg(opts, pkg)
  if (fetchResponse == null) return

  ctx.graph[pkg.dir] = createGraphNode(ctx, { pkg, fetchResponse, depIntegrityIsUnchanged })
}

async function checkPkgDirReuse (opts: LockfileToDepGraphOptions, check: PkgDirCheck): Promise<PkgDirReuse> {
  const dirExists = isUnchangedWithoutOptionalDeps(opts, check)
    ? await pathExistsOrLogMissing(check.dir)
    : undefined
  if (dirExists && !needsRebuild(check)) return 'up-to-date'

  if (
    check.isUnchanged &&
    equals(check.currentPkgSnapshot!.optionalDependencies, check.pkgSnapshot.optionalDependencies) &&
    await isPkgDirReusable(check, dirExists, () => {
      brokenModulesLogger.debug({ missing: check.dir })
    })
  ) {
    return 'reusable'
  }

  if (
    opts.enableGlobalVirtualStore &&
    !check.isDirectoryDep &&
    !opts.force &&
    await isPkgDirReusable(check, dirExists)
  ) {
    return 'reusable'
  }
  return 'needs-fetch'
}

function isUnchangedWithoutOptionalDeps (opts: LockfileToDepGraphOptions, check: PkgDirCheck): boolean {
  return check.isUnchanged &&
    isEmpty(check.currentPkgSnapshot!.optionalDependencies ?? {}) &&
    isEmpty(check.pkgSnapshot.optionalDependencies ?? {}) &&
    !opts.includeUnchangedDeps &&
    // The current lockfile vouches only for slots this project alone writes.
    // A global virtual store slot is shared, so another project's interrupted
    // install may have re-created it without its dependency links. Keeping it
    // in the graph relinks its children (https://github.com/pnpm/pnpm/issues/16642).
    !opts.enableGlobalVirtualStore
}

async function pathExistsOrLogMissing (dir: string): Promise<boolean> {
  const dirExists = await pathExists(dir)
  if (!dirExists) {
    brokenModulesLogger.debug({ missing: dir })
  }
  return dirExists
}

async function isPkgDirReusable (check: PkgDirCheck, dirExists: boolean | undefined, onMissingDir?: () => void): Promise<boolean> {
  if (dirExists ?? await pathExists(check.dir)) {
    return !needsRebuild(check)
  }
  onMissingDir?.()
  return false
}

function needsRebuild (check: PkgDirCheck): boolean {
  return check.mightNeedBuild && fs.existsSync(path.join(check.dir, '.pnpm-needs-build'))
}

async function fetchPkg (opts: LockfileToDepGraphOptions, pkg: LocatedPkg): Promise<Partial<FetchResponse> | undefined> {
  const { depPath, pkgSnapshot } = pkg.pkgMeta
  const resolution = pkgSnapshotToResolution(depPath, pkgSnapshot, pickRegistryContext(opts))
  if (!opts.omitResolvedProgress) {
    progressLogger.debug({ packageId: pkg.packageId, requester: opts.lockfileDir, status: 'resolved' })
  }

  try {
    return await opts.storeController.fetchPackage({
      allowBuild: opts.allowBuild,
      force: false,
      lockfileDir: opts.lockfileDir,
      ignoreScripts: opts.ignoreScripts,
      pkg: { name: pkg.pkgMeta.name, version: pkg.pkgMeta.version, id: pkg.packageId, resolution },
      supportedArchitectures: opts.supportedArchitectures,
    })
  } catch (err) {
    if (pkgSnapshot.optional) return undefined
    throw err
  }
}

function createGraphNode (
  ctx: BuildGraphContext,
  { pkg, fetchResponse, depIntegrityIsUnchanged }: {
    pkg: LocatedPkg
    fetchResponse: Partial<FetchResponse>
    depIntegrityIsUnchanged: boolean
  }
): DependenciesGraphNode {
  const { pkgIdWithPatchHash, name: pkgName, version: pkgVersion, depPath, pkgSnapshot } = pkg.pkgMeta
  return {
    children: {},
    pkgIdWithPatchHash,
    resolution: pkgSnapshot.resolution,
    depPath,
    dir: pkg.dir,
    fetching: fetchResponse.fetching,
    filesIndexFile: fetchResponse.filesIndexFile,
    forceImportPackage: !depIntegrityIsUnchanged,
    hasBin: pkgSnapshot.hasBin === true,
    hasBundledDependencies: pkgSnapshot.bundledDependencies != null,
    modules: pkg.modules,
    name: pkgName,
    version: pkgVersion,
    optional: !!pkgSnapshot.optional,
    optionalDependencies: new Set(Object.keys(pkgSnapshot.optionalDependencies ?? {})),
    patch: ctx.getPatchInfo(pkgName, pkgVersion),
  }
}

export function isIntegrityEqual (resolutionA?: LockfileResolution, resolutionB?: LockfileResolution): boolean {
  // The LockfileResolution type is a union, but it doesn't have a "tag"
  // field to perform a discriminant match on. Using a type assertion is
  // required to get the integrity field.
  const integrityA = (resolutionA as ({ integrity?: string } | undefined))?.integrity
  const integrityB = (resolutionB as ({ integrity?: string } | undefined))?.integrity

  return integrityA === integrityB
}
