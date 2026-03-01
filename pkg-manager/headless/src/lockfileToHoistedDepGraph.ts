import { pathExists } from 'path-exists'
import path from 'path'
import {
  type LockfileObject,
  type PackageSnapshot,
  type ProjectSnapshot,
} from '@pnpm/lockfile.fs'
import {
  nameVerFromPkgSnapshot,
  packageIdFromSnapshot,
  pkgSnapshotToResolution,
} from '@pnpm/lockfile.utils'
import { logger } from '@pnpm/logger'
import { type IncludedDependencies } from '@pnpm/modules-yaml'
import { packageIsInstallable } from '@pnpm/package-is-installable'
import { type PatchGroupRecord, getPatchInfo } from '@pnpm/patching.config'
import { safeReadPackageJsonFromDir } from '@pnpm/read-package-json'
import { type DepPath, type SupportedArchitectures, type ProjectId, type Registries, type AllowBuild } from '@pnpm/types'
import {
  type FetchPackageToStoreFunction,
  type StoreController,
} from '@pnpm/store-controller-types'
import { hoist, type HoistingLimits, type HoisterResult } from '@pnpm/real-hoist'
import * as dp from '@pnpm/dependency-path'
import {
  type DependenciesGraph,
  type DepHierarchy,
  type DirectDependenciesByImporterId,
  type LockfileToDepGraphResult,
} from '@pnpm/deps.graph-builder'

export interface LockfileToHoistedDepGraphOptions {
  allowBuild?: AllowBuild
  autoInstallPeers: boolean
  engineStrict: boolean
  force: boolean
  hoistingLimits?: HoistingLimits
  externalDependencies?: Set<string>
  importerIds: string[]
  include: IncludedDependencies
  ignoreScripts: boolean
  /**
   * When true, skip fetching local dependencies (file: protocol pointing to directories).
   * This is used by `pnpm fetch` which only downloads packages from the registry
   * and doesn't need local packages that won't be available (e.g., in Docker builds).
   */
  ignoreLocalPackages?: boolean
  currentHoistedLocations?: Record<string, string[]>
  lockfileDir: string
  modulesDir?: string
  nodeVersion: string
  pnpmVersion: string
  registries: Registries
  patchedDependencies?: PatchGroupRecord
  sideEffectsCacheRead: boolean
  skipped: Set<string>
  storeController: StoreController
  storeDir: string
  virtualStoreDir: string
  supportedArchitectures?: SupportedArchitectures
}

export async function lockfileToHoistedDepGraph (
  lockfile: LockfileObject,
  currentLockfile: LockfileObject | null,
  opts: LockfileToHoistedDepGraphOptions
): Promise<LockfileToDepGraphResult> {
  let prevGraph!: DependenciesGraph
  if (currentLockfile?.packages != null) {
    prevGraph = (await _lockfileToHoistedDepGraph(currentLockfile, {
      ...opts,
      force: true,
      skipped: new Set(),
    })).graph
  } else {
    prevGraph = {}
  }
  return {
    ...(await _lockfileToHoistedDepGraph(lockfile, opts)),
    prevGraph,
  }
}

async function _lockfileToHoistedDepGraph (
  lockfile: LockfileObject,
  opts: LockfileToHoistedDepGraphOptions
): Promise<Omit<LockfileToDepGraphResult, 'prevGraph'>> {
  const tree = hoist(lockfile, {
    hoistingLimits: opts.hoistingLimits,
    externalDependencies: opts.externalDependencies,
    autoInstallPeers: opts.autoInstallPeers,
  })
  const graph: DependenciesGraph = {}
  const modulesDir = path.join(opts.lockfileDir, opts.modulesDir ?? 'node_modules')
  const fetchDepsOpts = {
    ...opts,
    lockfile,
    graph,
    pkgLocationsByDepPath: {} as Record<string, string[]>,
    injectionTargetsByDepPath: new Map<string, string[]>(),
    hoistedLocations: {} as Record<string, string[]>,
  }
  const hierarchy = {
    [opts.lockfileDir]: await fetchDeps(fetchDepsOpts, modulesDir, tree.dependencies),
  }
  const directDependenciesByImporterId: DirectDependenciesByImporterId = {
    '.': directDepsMap(Object.keys(hierarchy[opts.lockfileDir]), graph),
  }
  const symlinkedDirectDependenciesByImporterId: DirectDependenciesByImporterId = { '.': {} }
  await Promise.all(
    Array.from(tree.dependencies).map(async (rootDep) => {
      const reference = Array.from(rootDep.references)[0]
      if (reference.startsWith('workspace:')) {
        const importerId = reference.replace('workspace:', '') as ProjectId
        const projectDir = path.join(opts.lockfileDir, importerId)
        const modulesDir = path.join(projectDir, 'node_modules')
        const nextHierarchy = (await fetchDeps(fetchDepsOpts, modulesDir, rootDep.dependencies))
        hierarchy[projectDir] = nextHierarchy

        const importer = lockfile.importers[importerId]
        const importerDir = path.join(opts.lockfileDir, importerId)
        symlinkedDirectDependenciesByImporterId[importerId] = pickLinkedDirectDeps(importer, importerDir, opts.include)
        directDependenciesByImporterId[importerId] = directDepsMap(Object.keys(nextHierarchy), graph)
      }
    })
  )
  return {
    directDependenciesByImporterId,
    extraVariantLinks: [],
    graph,
    hierarchy,
    symlinkedDirectDependenciesByImporterId,
    hoistedLocations: fetchDepsOpts.hoistedLocations,
    injectionTargetsByDepPath: fetchDepsOpts.injectionTargetsByDepPath,
  }
}

function directDepsMap (directDepDirs: string[], graph: DependenciesGraph): Record<string, string> {
  const acc: Record<string, string> = {}
  for (const dir of directDepDirs) {
    acc[graph[dir].alias!] = dir
  }
  return acc
}

function pickLinkedDirectDeps (
  importer: ProjectSnapshot,
  importerDir: string,
  include: IncludedDependencies
): Record<string, string> {
  const rootDeps = {
    ...(include.devDependencies ? importer.devDependencies : {}),
    ...(include.dependencies ? importer.dependencies : {}),
    ...(include.optionalDependencies ? importer.optionalDependencies : {}),
  }
  const directDeps: Record<string, string> = {}
  for (const alias in rootDeps) {
    const ref = rootDeps[alias]
    if (ref.startsWith('link:')) {
      directDeps[alias] = path.resolve(importerDir, ref.slice(5))
    }
  }
  return directDeps
}

async function fetchDeps (
  opts: {
    graph: DependenciesGraph
    lockfile: LockfileObject
    pkgLocationsByDepPath: Record<string, string[]>
    injectionTargetsByDepPath: Map<string, string[]>
    hoistedLocations: Record<string, string[]>
  } & LockfileToHoistedDepGraphOptions,
  modules: string,
  deps: Set<HoisterResult>
): Promise<DepHierarchy> {
  const depHierarchy: Record<string, DepHierarchy> = {}
  await Promise.all(Array.from(deps).map(async (dep) => {
    const depPath = Array.from(dep.references)[0] as DepPath
    if (opts.skipped.has(depPath) || depPath.startsWith('workspace:')) return
    const pkgSnapshot = opts.lockfile.packages![depPath]
    if (!pkgSnapshot) {
      // it is a link
      return
    }
    const { name: pkgName, version: pkgVersion } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
    const packageId = packageIdFromSnapshot(depPath, pkgSnapshot)
    const pkgIdWithPatchHash = dp.getPkgIdWithPatchHash(depPath)

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
        supportedArchitectures: opts.supportedArchitectures,
      }) === false
    ) {
      opts.skipped.add(depPath)
      return
    }

    const isDirectoryDep = 'directory' in pkgSnapshot.resolution && pkgSnapshot.resolution.directory != null
    if (isDirectoryDep && opts.ignoreLocalPackages) {
      logger.info({
        message: `Skipping local dependency ${pkgName}@${pkgVersion} (file: protocol)`,
        prefix: opts.lockfileDir,
      })
      return
    }

    const dir = path.join(modules, dep.name)
    const depLocation = path.relative(opts.lockfileDir, dir)
    const resolution = pkgSnapshotToResolution(depPath, pkgSnapshot, opts.registries)
    let fetchResponse!: ReturnType<FetchPackageToStoreFunction>
    // We check for the existence of the package inside node_modules.
    // It will only be missing if the user manually removed it.
    // That shouldn't normally happen but Bit CLI does remove node_modules in component directories:
    // https://github.com/teambit/bit/blob/5e1eed7cd122813ad5ea124df956ee89d661d770/scopes/dependencies/dependency-resolver/dependency-installer.ts#L169
    //
    // We also verify that the package that is present has the expected version.
    // This check is required because there is no guarantee the modules manifest and current lockfile were
    // successfully saved after node_modules was changed during installation.
    const skipFetch = opts.currentHoistedLocations?.[depPath]?.includes(depLocation) &&
      await dirHasPackageJsonWithVersion(path.join(opts.lockfileDir, depLocation), pkgVersion)
    const pkgResolution = {
      id: packageId,
      resolution,
      name: pkgName,
      version: pkgVersion,
    }
    if (skipFetch) {
      const { filesIndexFile } = opts.storeController.getFilesIndexFilePath({
        ignoreScripts: opts.ignoreScripts,
        pkg: pkgResolution,
      })
      fetchResponse = { filesIndexFile } as unknown as ReturnType<FetchPackageToStoreFunction>
    } else {
      try {
        fetchResponse = opts.storeController.fetchPackage({
          allowBuild: opts.allowBuild,
          force: false,
          lockfileDir: opts.lockfileDir,
          ignoreScripts: opts.ignoreScripts,
          pkg: pkgResolution,
          supportedArchitectures: opts.supportedArchitectures,
        }) as unknown as ReturnType<FetchPackageToStoreFunction>
        if (fetchResponse instanceof Promise) fetchResponse = await fetchResponse
      } catch (err: unknown) {
        if (pkgSnapshot.optional) return
        throw err
      }
    }
    opts.graph[dir] = {
      alias: dep.name,
      children: {},
      depPath,
      pkgIdWithPatchHash,
      dir,
      fetching: fetchResponse.fetching,
      filesIndexFile: fetchResponse.filesIndexFile,
      hasBin: pkgSnapshot.hasBin === true,
      hasBundledDependencies: pkgSnapshot.bundledDependencies != null,
      modules,
      name: pkgName,
      version: pkgVersion,
      optional: !!pkgSnapshot.optional,
      optionalDependencies: new Set(Object.keys(pkgSnapshot.optionalDependencies ?? {})),
      patch: getPatchInfo(opts.patchedDependencies, pkgName, pkgVersion),
      resolution: pkgSnapshot.resolution,
    }
    if (!opts.pkgLocationsByDepPath[depPath]) {
      opts.pkgLocationsByDepPath[depPath] = []
    }
    opts.pkgLocationsByDepPath[depPath].push(dir)
    // Track directory deps for injected workspace packages
    if ('directory' in pkgSnapshot.resolution && pkgSnapshot.resolution.directory != null) {
      const locations = opts.injectionTargetsByDepPath.get(depPath)
      if (locations) {
        locations.push(dir)
      } else {
        opts.injectionTargetsByDepPath.set(depPath, [dir])
      }
    }
    depHierarchy[dir] = await fetchDeps(opts, path.join(dir, 'node_modules'), dep.dependencies)
    if (!opts.hoistedLocations[depPath]) {
      opts.hoistedLocations[depPath] = []
    }
    opts.hoistedLocations[depPath].push(depLocation)
    opts.graph[dir].children = getChildren(pkgSnapshot, opts.pkgLocationsByDepPath, opts)
  }))
  return depHierarchy
}

async function dirHasPackageJsonWithVersion (dir: string, expectedVersion?: string): Promise<boolean> {
  if (!expectedVersion) return pathExists(dir)
  try {
    const manifest = await safeReadPackageJsonFromDir(dir)
    return manifest?.version === expectedVersion
  } catch (err: unknown) {
    if ((err as NodeJS.ErrnoException)?.code === 'ENOENT') {
      return pathExists(dir)
    }
    throw err
  }
}

function getChildren (
  pkgSnapshot: PackageSnapshot,
  pkgLocationsByDepPath: Record<string, string[]>,
  opts: { include: IncludedDependencies }
): Record<string, string> {
  const allDeps = {
    ...pkgSnapshot.dependencies,
    ...(opts.include.optionalDependencies ? pkgSnapshot.optionalDependencies : {}),
  }
  const children: Record<string, string> = {}
  for (const [childName, childRef] of Object.entries(allDeps)) {
    const childDepPath = dp.refToRelative(childRef, childName)
    if (childDepPath && pkgLocationsByDepPath[childDepPath]) {
      children[childName] = pkgLocationsByDepPath[childDepPath][0]
    }
  }
  return children
}
