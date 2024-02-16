import path from 'path'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import {
  progressLogger,
} from '@pnpm/core-loggers'
import {
  type Lockfile,
  type PackageSnapshot,
} from '@pnpm/lockfile-file'
import {
  nameVerFromPkgSnapshot,
  packageIdFromSnapshot,
  pkgSnapshotToResolution,
  refIsLocalDirectory,
} from '@pnpm/lockfile-utils'
import { logger } from '@pnpm/logger'
import { type IncludedDependencies } from '@pnpm/modules-yaml'
import { packageIsInstallable } from '@pnpm/package-is-installable'
import { type SupportedArchitectures, type PatchFile, type Registries } from '@pnpm/types'
import {
  type PkgRequestFetchResult,
  type FetchPackageToStoreFunction,
  type StoreController,
} from '@pnpm/store-controller-types'
import * as dp from '@pnpm/dependency-path'
import pathExists from 'path-exists'
import equals from 'ramda/src/equals'
import isEmpty from 'ramda/src/isEmpty'

const brokenModulesLogger = logger('_broken_node_modules')

export interface DependenciesGraphNode {
  alias?: string // this is populated in HoistedDepGraphOnly
  hasBundledDependencies: boolean
  modules: string
  name: string
  fetching?: () => Promise<PkgRequestFetchResult>
  dir: string
  children: Record<string, string>
  optionalDependencies: Set<string>
  optional: boolean
  depPath: string // this option is only needed for saving pendingBuild when running with --ignore-scripts flag
  isBuilt?: boolean
  requiresBuild: boolean
  prepare: boolean
  hasBin: boolean
  filesIndexFile?: string
  patchFile?: PatchFile
}

export interface DependenciesGraph {
  [depPath: string]: DependenciesGraphNode
}

export interface LockfileToDepGraphOptions {
  autoInstallPeers: boolean
  engineStrict: boolean
  force: boolean
  importerIds: string[]
  include: IncludedDependencies
  ignoreScripts: boolean
  lockfileDir: string
  nodeVersion: string
  pnpmVersion: string
  patchedDependencies?: Record<string, PatchFile>
  registries: Registries
  sideEffectsCacheRead: boolean
  skipped: Set<string>
  storeController: StoreController
  storeDir: string
  virtualStoreDir: string
  supportedArchitectures?: SupportedArchitectures
}

export interface DirectDependenciesByImporterId {
  [importerId: string]: { [alias: string]: string }
}

export interface DepHierarchy {
  [depPath: string]: Record<string, DepHierarchy>
}

export interface LockfileToDepGraphResult {
  directDependenciesByImporterId: DirectDependenciesByImporterId
  graph: DependenciesGraph
  hierarchy?: DepHierarchy
  hoistedLocations?: Record<string, string[]>
  symlinkedDirectDependenciesByImporterId?: DirectDependenciesByImporterId
  prevGraph?: DependenciesGraph
  pkgLocationsByDepPath?: Record<string, string[]>
}

export async function lockfileToDepGraph (
  lockfile: Lockfile,
  currentLockfile: Lockfile | null,
  opts: LockfileToDepGraphOptions
): Promise<LockfileToDepGraphResult> {
  const currentPackages = currentLockfile?.packages ?? {}
  const graph: DependenciesGraph = {}
  const directDependenciesByImporterId: DirectDependenciesByImporterId = {}
  if (lockfile.packages != null) {
    const pkgSnapshotByLocation: Record<string, PackageSnapshot> = {}
    await Promise.all(
      Object.entries(lockfile.packages).map(async ([depPath, pkgSnapshot]) => {
        if (opts.skipped.has(depPath)) return
        // TODO: optimize. This info can be already returned by pkgSnapshotToResolution()
        const { name: pkgName, version: pkgVersion } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
        const modules = path.join(opts.virtualStoreDir, dp.depPathToFilename(depPath), 'node_modules')
        const packageId = packageIdFromSnapshot(depPath, pkgSnapshot)

        const pkg = {
          name: pkgName,
          version: pkgVersion,
          engines: pkgSnapshot.engines,
          cpu: pkgSnapshot.cpu,
          os: pkgSnapshot.os,
          libc: pkgSnapshot.libc,
        }
        if (!opts.force &&
          packageIsInstallable(packageId, pkg, {
            engineStrict: opts.engineStrict,
            lockfileDir: opts.lockfileDir,
            nodeVersion: opts.nodeVersion,
            optional: pkgSnapshot.optional === true,
            pnpmVersion: opts.pnpmVersion,
            supportedArchitectures: opts.supportedArchitectures,
          }) === false
        ) {
          opts.skipped.add(depPath)
          return
        }
        const dir = path.join(modules, pkgName)
        const depIsPresent = !refIsLocalDirectory(depPath) &&
          currentPackages[depPath] && equals(currentPackages[depPath].dependencies, lockfile.packages![depPath].dependencies)
        let dirExists: boolean | undefined
        if (
          depIsPresent && isEmpty(currentPackages[depPath].optionalDependencies ?? {}) &&
          isEmpty(lockfile.packages![depPath].optionalDependencies ?? {})
        ) {
          dirExists = await pathExists(dir)
          if (dirExists) {
            return
          }

          brokenModulesLogger.debug({
            missing: dir,
          })
        }
        let fetchResponse!: Partial<ReturnType<FetchPackageToStoreFunction>>
        if (depIsPresent && equals(currentPackages[depPath].optionalDependencies, lockfile.packages![depPath].optionalDependencies)) {
          if (dirExists ?? await pathExists(dir)) {
            fetchResponse = {}
          } else {
            brokenModulesLogger.debug({
              missing: dir,
            })
          }
        }
        if (!fetchResponse) {
          const resolution = pkgSnapshotToResolution(depPath, pkgSnapshot, opts.registries)
          progressLogger.debug({
            packageId,
            requester: opts.lockfileDir,
            status: 'resolved',
          })
          try {
            fetchResponse = opts.storeController.fetchPackage({
              force: false,
              lockfileDir: opts.lockfileDir,
              ignoreScripts: opts.ignoreScripts,
              pkg: {
                id: packageId,
                resolution,
              },
              expectedPkg: {
                name: pkgName,
                version: pkgVersion,
              },
            }) as any // eslint-disable-line
            if (fetchResponse instanceof Promise) fetchResponse = await fetchResponse
          } catch (err: any) { // eslint-disable-line
            if (pkgSnapshot.optional) return
            throw err
          }
        }
        graph[dir] = {
          children: {},
          depPath,
          dir,
          fetching: fetchResponse.fetching,
          filesIndexFile: fetchResponse.filesIndexFile,
          hasBin: pkgSnapshot.hasBin === true,
          hasBundledDependencies: pkgSnapshot.bundledDependencies != null,
          modules,
          name: pkgName,
          optional: !!pkgSnapshot.optional,
          optionalDependencies: new Set(Object.keys(pkgSnapshot.optionalDependencies ?? {})),
          prepare: pkgSnapshot.prepare === true,
          requiresBuild: pkgSnapshot.requiresBuild === true,
          patchFile: opts.patchedDependencies?.[`${pkgName}@${pkgVersion}`],
        }
        pkgSnapshotByLocation[dir] = pkgSnapshot
      })
    )
    const ctx = {
      force: opts.force,
      graph,
      lockfileDir: opts.lockfileDir,
      pkgSnapshotsByDepPaths: lockfile.packages,
      registries: opts.registries,
      sideEffectsCacheRead: opts.sideEffectsCacheRead,
      skipped: opts.skipped,
      storeController: opts.storeController,
      storeDir: opts.storeDir,
      virtualStoreDir: opts.virtualStoreDir,
    }
    for (const [dir, node] of Object.entries(graph)) {
      const pkgSnapshot = pkgSnapshotByLocation[dir]
      const allDeps = {
        ...pkgSnapshot.dependencies,
        ...(opts.include.optionalDependencies ? pkgSnapshot.optionalDependencies : {}),
      }

      const peerDeps = pkgSnapshot.peerDependencies ? new Set(Object.keys(pkgSnapshot.peerDependencies)) : null
      node.children = getChildrenPaths(ctx, allDeps, peerDeps, '.')
    }
    for (const importerId of opts.importerIds) {
      const projectSnapshot = lockfile.importers[importerId]
      const rootDeps = {
        ...(opts.include.devDependencies ? projectSnapshot.devDependencies : {}),
        ...(opts.include.dependencies ? projectSnapshot.dependencies : {}),
        ...(opts.include.optionalDependencies ? projectSnapshot.optionalDependencies : {}),
      }
      directDependenciesByImporterId[importerId] = getChildrenPaths(ctx, rootDeps, null, importerId)
    }
  }
  return { graph, directDependenciesByImporterId }
}

function getChildrenPaths (
  ctx: {
    graph: DependenciesGraph
    force: boolean
    registries: Registries
    virtualStoreDir: string
    storeDir: string
    skipped: Set<string>
    pkgSnapshotsByDepPaths: Record<string, PackageSnapshot>
    lockfileDir: string
    sideEffectsCacheRead: boolean
    storeController: StoreController
  },
  allDeps: { [alias: string]: string },
  peerDeps: Set<string> | null,
  importerId: string
) {
  const children: { [alias: string]: string } = {}
  for (const [alias, ref] of Object.entries(allDeps)) {
    const childDepPath = dp.refToRelative(ref, alias)
    if (childDepPath === null) {
      children[alias] = path.resolve(ctx.lockfileDir, importerId, ref.slice(5))
      continue
    }
    const childRelDepPath = dp.refToRelative(ref, alias) as string
    const childPkgSnapshot = ctx.pkgSnapshotsByDepPaths[childRelDepPath]
    if (ctx.graph[childRelDepPath]) {
      children[alias] = ctx.graph[childRelDepPath].dir
    } else if (childPkgSnapshot) {
      if (ctx.skipped.has(childRelDepPath)) continue
      const pkgName = nameVerFromPkgSnapshot(childRelDepPath, childPkgSnapshot).name
      children[alias] = path.join(ctx.virtualStoreDir, dp.depPathToFilename(childRelDepPath), 'node_modules', pkgName)
    } else if (ref.indexOf('file:') === 0) {
      children[alias] = path.resolve(ctx.lockfileDir, ref.slice(5))
    } else if (!ctx.skipped.has(childRelDepPath) && ((peerDeps == null) || !peerDeps.has(alias))) {
      throw new Error(`${childRelDepPath} not found in ${WANTED_LOCKFILE}`)
    }
  }
  return children
}
