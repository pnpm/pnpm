import { promises as fs } from 'fs'
import path from 'path'
import { calcDepState, type DepsStateCache } from '@pnpm/calc-dep-state'
import {
  progressLogger,
  stageLogger,
  statsLogger,
} from '@pnpm/core-loggers'
import {
  filterLockfileByImporters,
} from '@pnpm/lockfile.filtering'
import { linkDirectDeps } from '@pnpm/pkg-manager.direct-dep-linker'
import { type InstallationResultStats } from '@pnpm/headless'
import { hoist, type HoistedWorkspaceProject } from '@pnpm/hoist'
import { type LockfileObject } from '@pnpm/lockfile.fs'
import { logger } from '@pnpm/logger'
import { prune } from '@pnpm/modules-cleaner'
import { type IncludedDependencies } from '@pnpm/modules-yaml'
import {
  type DependenciesGraph,
  type DependenciesGraphNode,
  type LinkedDependency,
} from '@pnpm/resolve-dependencies'
import { getExtraVariantDescriptors } from '@pnpm/package-requester'
import { type FetchResponse, type StoreController, type TarballResolution } from '@pnpm/store-controller-types'
import { symlinkDependency } from '@pnpm/symlink-dependency'
import {
  type AllowBuild,
  type DepPath,
  type HoistedDependencies,
  type PkgIdWithPatchHash,
  type Registries,
  type ProjectId,
  type SupportedArchitectures,
} from '@pnpm/types'
import { symlinkAllModules } from '@pnpm/worker'
import pLimit from 'p-limit'
import { pathExists } from 'path-exists'
import { equals, isEmpty, difference, pick, pickBy, props } from 'ramda'
import { type ImporterToUpdate } from './index.js'

const brokenModulesLogger = logger('_broken_node_modules')

export interface LinkPackagesOptions {
  allowBuild?: AllowBuild
  currentLockfile: LockfileObject
  dedupeDirectDeps: boolean
  dependenciesByProjectId: Record<string, Map<string, DepPath>>
  disableRelinkLocalDirDeps?: boolean
  force: boolean
  depsStateCache: DepsStateCache
  extraNodePaths: string[]
  hoistedDependencies: HoistedDependencies
  hoistedModulesDir: string
  hoistPattern?: string[]
  ignoreScripts: boolean
  publicHoistPattern?: string[]
  include: IncludedDependencies
  linkedDependenciesByProjectId: Record<string, LinkedDependency[]>
  lockfileDir: string
  makePartialCurrentLockfile: boolean
  outdatedDependencies: Record<string, string>
  pruneStore: boolean
  pruneVirtualStore: boolean
  registries: Registries
  rootModulesDir: string
  sideEffectsCacheRead: boolean
  symlink: boolean
  skipped: Set<DepPath>
  storeController: StoreController
  supportedArchitectures?: SupportedArchitectures
  virtualStoreDir: string
  virtualStoreDirMaxLength: number
  wantedLockfile: LockfileObject
  wantedToBeSkippedPackageIds: Set<string>
  hoistWorkspacePackages?: boolean
}

export interface LinkPackagesResult {
  currentLockfile: LockfileObject
  newDepPaths: DepPath[]
  newHoistedDependencies: HoistedDependencies
  removedDepPaths: Set<string>
  stats: InstallationResultStats
}

export async function linkPackages (projects: ImporterToUpdate[], depGraph: DependenciesGraph, opts: LinkPackagesOptions): Promise<LinkPackagesResult> {
  let depNodes = Object.values(depGraph).filter(({ depPath, id }) => {
    if (((opts.wantedLockfile.packages?.[depPath]) != null) && !opts.wantedLockfile.packages[depPath].optional) {
      opts.skipped.delete(depPath)
      return true
    }
    if (opts.wantedToBeSkippedPackageIds.has(id)) {
      opts.skipped.add(depPath)
      return false
    }
    opts.skipped.delete(depPath)
    return true
  })
  if (!opts.include.dependencies) {
    depNodes = depNodes.filter(({ dev, optional }) => dev || optional)
  }
  if (!opts.include.devDependencies) {
    depNodes = depNodes.filter(({ optional, prod }) => prod || optional)
  }
  if (!opts.include.optionalDependencies) {
    depNodes = depNodes.filter(({ optional }) => !optional)
  }
  depGraph = Object.fromEntries(depNodes.map((depNode) => [depNode.depPath, depNode]))

  // Inject extra variant nodes for multi-architecture support
  const extraVariantNodes: DependenciesGraphNode[] = []
  const extraVariantSymlinks: Array<{ alias: string, dir: string, modulesDir: string }> = []
  if (opts.supportedArchitectures) {
    const extraPromises: Array<Promise<void>> = []
    for (const node of Object.values(depGraph)) {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      if ((node.resolution as any).type !== 'variations') continue
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const variants = (node.resolution as any).variants
      const descriptors = getExtraVariantDescriptors(variants, opts.supportedArchitectures, {
        primaryDir: node.dir,
        packageName: node.name,
        parentDepPath: node.depPath,
      })
      if (descriptors.length === 0) continue
      const projectModulesDirs: string[] = []
      for (const project of projects) {
        const deps = opts.dependenciesByProjectId[project.id]
        if (deps && Array.from(deps.values()).includes(node.depPath)) {
          projectModulesDirs.push(project.modulesDir)
        }
      }
      for (const descriptor of descriptors) {
        extraPromises.push((async () => {
          let fetchResponse: FetchResponse
          try {
            fetchResponse = await opts.storeController.fetchPackage({
              allowBuild: opts.allowBuild,
              force: false,
              lockfileDir: opts.lockfileDir,
              ignoreScripts: opts.ignoreScripts,
              pkg: {
                name: descriptor.variantName,
                version: node.version,
                id: descriptor.variantDepPath,
                resolution: node.resolution,
              },
              supportedArchitectures: {
                os: [descriptor.os],
                cpu: [descriptor.cpu],
                libc: descriptor.libc ? [descriptor.libc] : undefined,
              },
            })
          } catch {
            return
          }
          const syntheticNode: DependenciesGraphNode = {
            ...node,
            id: descriptor.variantDepPath as unknown as typeof node.id,
            name: descriptor.variantName,
            dir: descriptor.variantDir,
            modules: descriptor.variantModules,
            depPath: descriptor.variantDepPath as DepPath,
            pkgIdWithPatchHash: descriptor.variantDepPath as unknown as PkgIdWithPatchHash,
            fetching: fetchResponse.fetching,
            filesIndexFile: fetchResponse.filesIndexFile,
            hasBin: false,
            children: {},
            optional: false,
            optionalDependencies: new Set(),
            patch: undefined,
          }
          depGraph[descriptor.variantDepPath as DepPath] = syntheticNode
          extraVariantNodes.push(syntheticNode)
          for (const modulesDir of projectModulesDirs) {
            extraVariantSymlinks.push({ alias: descriptor.variantName, dir: descriptor.variantDir, modulesDir })
          }
        })())
      }
    }
    await Promise.all(extraPromises)
  }

  const removedDepPaths = await prune(projects, {
    currentLockfile: opts.currentLockfile,
    dedupeDirectDeps: opts.dedupeDirectDeps,
    hoistedDependencies: opts.hoistedDependencies,
    hoistedModulesDir: (opts.hoistPattern != null) ? opts.hoistedModulesDir : undefined,
    include: opts.include,
    lockfileDir: opts.lockfileDir,
    pruneStore: opts.pruneStore,
    pruneVirtualStore: opts.pruneVirtualStore,
    publicHoistedModulesDir: (opts.publicHoistPattern != null) ? opts.rootModulesDir : undefined,
    skipped: opts.skipped,
    storeController: opts.storeController,
    virtualStoreDir: opts.virtualStoreDir,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    wantedLockfile: opts.wantedLockfile,
  })

  stageLogger.debug({
    prefix: opts.lockfileDir,
    stage: 'importing_started',
  })

  const projectIds = projects.map(({ id }) => id)
  const filterOpts = {
    include: opts.include,
    registries: opts.registries,
    skipped: opts.skipped,
  }
  const newCurrentLockfile = filterLockfileByImporters(opts.wantedLockfile, projectIds, {
    ...filterOpts,
    failOnMissingDependencies: true,
    skipped: new Set(),
  })
  const { newDepPaths, added } = await linkNewPackages(
    filterLockfileByImporters(opts.currentLockfile, projectIds, {
      ...filterOpts,
      failOnMissingDependencies: false,
    }),
    newCurrentLockfile,
    depGraph,
    {
      allowBuild: opts.allowBuild,
      disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
      force: opts.force,
      depsStateCache: opts.depsStateCache,
      ignoreScripts: opts.ignoreScripts,
      lockfileDir: opts.lockfileDir,
      optional: opts.include.optionalDependencies,
      sideEffectsCacheRead: opts.sideEffectsCacheRead,
      symlink: opts.symlink,
      skipped: opts.skipped,
      storeController: opts.storeController,
      virtualStoreDir: opts.virtualStoreDir,
    }
  )

  // Import extra variant packages (not in lockfile, handled separately)
  if (extraVariantNodes.length > 0) {
    await Promise.all(extraVariantNodes.map(async (depNode) => fs.mkdir(depNode.modules, { recursive: true })))
    await linkAllPkgs(opts.storeController, extraVariantNodes, {
      allowBuild: opts.allowBuild,
      depGraph,
      depsStateCache: opts.depsStateCache,
      disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
      force: opts.force,
      ignoreScripts: opts.ignoreScripts,
      lockfileDir: opts.lockfileDir,
      sideEffectsCacheRead: opts.sideEffectsCacheRead,
    })
  }

  // Create top-level symlinks for extra variants
  if (opts.symlink && extraVariantSymlinks.length > 0) {
    await Promise.all(
      extraVariantSymlinks.map(({ alias, dir, modulesDir }) =>
        symlinkDependency(dir, modulesDir, alias)
      )
    )
  }

  stageLogger.debug({
    prefix: opts.lockfileDir,
    stage: 'importing_done',
  })

  let currentLockfile: LockfileObject
  const allImportersIncluded = equals(projectIds.sort(), Object.keys(opts.wantedLockfile.importers).sort())
  if (
    opts.makePartialCurrentLockfile ||
    !allImportersIncluded
  ) {
    const packages = opts.currentLockfile.packages ?? {}
    if (opts.wantedLockfile.packages != null) {
      for (const depPath in opts.wantedLockfile.packages) { // eslint-disable-line:forin
        if (depGraph[depPath as DepPath]) {
          packages[depPath as DepPath] = opts.wantedLockfile.packages[depPath as DepPath]
        }
      }
    }
    const projects = {
      ...opts.currentLockfile.importers,
      ...pick(projectIds, opts.wantedLockfile.importers),
    }
    currentLockfile = filterLockfileByImporters(
      {
        ...opts.wantedLockfile,
        importers: projects,
        packages,
      },
      Object.keys(projects) as ProjectId[], {
        ...filterOpts,
        failOnMissingDependencies: false,
        skipped: new Set(),
      }
    )
  } else if (
    opts.include.dependencies &&
    opts.include.devDependencies &&
    opts.include.optionalDependencies &&
    opts.skipped.size === 0
  ) {
    currentLockfile = opts.wantedLockfile
  } else {
    currentLockfile = newCurrentLockfile
  }

  let newHoistedDependencies!: HoistedDependencies
  if (opts.hoistPattern == null && opts.publicHoistPattern == null) {
    newHoistedDependencies = {}
  } else if (newDepPaths.length > 0 || removedDepPaths.size > 0) {
    newHoistedDependencies = {
      ...opts.hoistedDependencies,
      ...await hoist({
        extraNodePath: opts.extraNodePaths,
        graph: depGraph,
        directDepsByImporterId: {
          ...opts.dependenciesByProjectId,
          '.': new Map(Array.from(opts.dependenciesByProjectId['.']?.entries() ?? []).filter(([alias]) => {
            return newCurrentLockfile.importers['.' as ProjectId].specifiers[alias]
          })),
        },
        importerIds: projectIds,
        privateHoistedModulesDir: opts.hoistedModulesDir,
        privateHoistPattern: opts.hoistPattern ?? [],
        publicHoistedModulesDir: opts.rootModulesDir,
        publicHoistPattern: opts.publicHoistPattern ?? [],
        virtualStoreDir: opts.virtualStoreDir,
        virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
        hoistedWorkspacePackages: opts.hoistWorkspacePackages
          ? projects.reduce((hoistedWorkspacePackages, project) => {
            if (project.manifest.name && project.id !== '.') {
              hoistedWorkspacePackages[project.id] = {
                dir: project.rootDir,
                name: project.manifest.name,
              }
            }
            return hoistedWorkspacePackages
          }, {} as Record<string, HoistedWorkspaceProject>)
          : undefined,
        skipped: opts.skipped,
      }),
    }
  } else {
    newHoistedDependencies = opts.hoistedDependencies
  }

  let linkedToRoot = 0
  if (opts.symlink) {
    const projectsToLink = Object.fromEntries(await Promise.all(
      projects.map(async ({ id, manifest, modulesDir, rootDir }) => {
        const deps = opts.dependenciesByProjectId[id]
        const importerFromLockfile = newCurrentLockfile.importers[id]
        return [id, {
          dir: rootDir,
          modulesDir,
          dependencies: await Promise.all([
            ...Array.from(deps.entries())
              .filter(([rootAlias]) => importerFromLockfile.specifiers[rootAlias])
              .map(([rootAlias, depPath]) => ({ rootAlias, depGraphNode: depGraph[depPath] }))
              .filter(({ depGraphNode }) => depGraphNode)
              .map(async ({ rootAlias, depGraphNode }) => {
                const isDev = Boolean(manifest.devDependencies?.[depGraphNode.name])
                const isOptional = Boolean(manifest.optionalDependencies?.[depGraphNode.name])
                return {
                  alias: rootAlias,
                  name: depGraphNode.name,
                  version: depGraphNode.version,
                  dir: depGraphNode.dir,
                  id: depGraphNode.id,
                  dependencyType: (isDev && 'dev' || isOptional && 'optional' || 'prod') as 'dev' | 'optional' | 'prod',
                  latest: opts.outdatedDependencies[depGraphNode.id],
                  isExternalLink: false,
                }
              }),
            ...opts.linkedDependenciesByProjectId[id].map(async (linkedDependency) => {
              const dir = resolvePath(rootDir, linkedDependency.resolution.directory)
              return {
                alias: linkedDependency.alias,
                name: linkedDependency.name,
                version: linkedDependency.version,
                dir,
                id: linkedDependency.resolution.directory,
                dependencyType: (linkedDependency.dev && 'dev' || linkedDependency.optional && 'optional' || 'prod') as 'dev' | 'optional' | 'prod',
                isExternalLink: true,
              }
            }),
          ]),
        }]
      }))
    )
    linkedToRoot = await linkDirectDeps(projectsToLink, { dedupe: opts.dedupeDirectDeps })
  }

  return {
    currentLockfile,
    newDepPaths,
    newHoistedDependencies,
    removedDepPaths,
    stats: {
      added,
      removed: removedDepPaths.size,
      linkedToRoot,
    },
  }
}

const isAbsolutePath = /^\/|^[A-Z]:/i

// This function is copied from @pnpm/local-resolver
function resolvePath (where: string, spec: string): string {
  if (isAbsolutePath.test(spec)) return spec
  return path.resolve(where, spec)
}

interface LinkNewPackagesOptions {
  allowBuild?: AllowBuild
  depsStateCache: DepsStateCache
  disableRelinkLocalDirDeps?: boolean
  force: boolean
  optional: boolean
  ignoreScripts: boolean
  lockfileDir: string
  sideEffectsCacheRead: boolean
  symlink: boolean
  skipped: Set<DepPath>
  storeController: StoreController
  virtualStoreDir: string
}

interface LinkNewPackagesResult {
  newDepPaths: DepPath[]
  added: number
}

async function linkNewPackages (
  currentLockfile: LockfileObject,
  wantedLockfile: LockfileObject,
  depGraph: DependenciesGraph,
  opts: LinkNewPackagesOptions
): Promise<LinkNewPackagesResult> {
  const wantedRelDepPaths = difference(Object.keys(wantedLockfile.packages ?? {}) as DepPath[], Array.from(opts.skipped))

  let newDepPathsSet: Set<DepPath>
  if (opts.force) {
    newDepPathsSet = new Set(
      wantedRelDepPaths
        // when installing a new package, not all the nodes are analyzed
        // just skip the ones that are in the lockfile but were not analyzed
        .filter((depPath) => depGraph[depPath])
    )
  } else {
    newDepPathsSet = await selectNewFromWantedDeps(wantedRelDepPaths, currentLockfile, depGraph)
  }

  const added = newDepPathsSet.size
  statsLogger.debug({
    added,
    prefix: opts.lockfileDir,
  })

  const existingWithUpdatedDeps = []
  if (!opts.force && (currentLockfile.packages != null) && (wantedLockfile.packages != null)) {
    // add subdependencies that have been updated
    // TODO: no need to relink everything. Can be relinked only what was changed
    for (const depPath of wantedRelDepPaths) {
      if (currentLockfile.packages[depPath] &&
        (!equals(currentLockfile.packages[depPath].dependencies, wantedLockfile.packages[depPath].dependencies) ||
        !isEmpty(currentLockfile.packages[depPath].optionalDependencies ?? {}) ||
        !isEmpty(wantedLockfile.packages[depPath].optionalDependencies ?? {}))
      ) {
        // TODO: come up with a test that triggers the usecase of depGraph[depPath] undefined
        // see related issue: https://github.com/pnpm/pnpm/issues/870
        if (depGraph[depPath] && !newDepPathsSet.has(depPath)) {
          existingWithUpdatedDeps.push(depGraph[depPath])
        }
      }
    }
  }

  if (!newDepPathsSet.size && (existingWithUpdatedDeps.length === 0)) return { newDepPaths: [], added }

  const newDepPaths = Array.from(newDepPathsSet)

  const newPkgs = props<DepPath, DependenciesGraphNode>(newDepPaths, depGraph)

  await Promise.all(newPkgs.map(async (depNode) => fs.mkdir(depNode.modules, { recursive: true })))
  await Promise.all([
    !opts.symlink
      ? Promise.resolve()
      : linkAllModules([...newPkgs, ...existingWithUpdatedDeps], depGraph, {
        lockfileDir: opts.lockfileDir,
        optional: opts.optional,
      }),
    linkAllPkgs(opts.storeController, newPkgs, {
      allowBuild: opts.allowBuild,
      depGraph,
      depsStateCache: opts.depsStateCache,
      disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
      force: opts.force,
      ignoreScripts: opts.ignoreScripts,
      lockfileDir: opts.lockfileDir,
      sideEffectsCacheRead: opts.sideEffectsCacheRead,
    }),
  ])

  return { newDepPaths, added }
}

async function selectNewFromWantedDeps (
  wantedRelDepPaths: DepPath[],
  currentLockfile: LockfileObject,
  depGraph: DependenciesGraph
): Promise<Set<DepPath>> {
  const newDeps = new Set<DepPath>()
  const prevDeps = currentLockfile.packages ?? {}
  await Promise.all(
    wantedRelDepPaths.map(
      async (depPath) => {
        const depNode = depGraph[depPath]
        if (!depNode) return
        const prevDep = prevDeps[depPath]
        if (
          prevDep &&
          // Local file should always be treated as a new dependency
          // https://github.com/pnpm/pnpm/issues/5381
          depNode.resolution.type !== 'directory' &&
          (depNode.resolution as TarballResolution).integrity === (prevDep.resolution as TarballResolution).integrity
        ) {
          if (await pathExists(depNode.dir)) {
            return
          }
          brokenModulesLogger.debug({
            missing: depNode.dir,
          })
        }
        newDeps.add(depPath)
      }
    )
  )
  return newDeps
}

const limitLinking = pLimit(16)

async function linkAllPkgs (
  storeController: StoreController,
  depNodes: DependenciesGraphNode[],
  opts: {
    allowBuild?: AllowBuild
    depGraph: DependenciesGraph
    depsStateCache: DepsStateCache
    disableRelinkLocalDirDeps?: boolean
    force: boolean
    ignoreScripts: boolean
    lockfileDir: string
    sideEffectsCacheRead: boolean
  }
): Promise<void> {
  await Promise.all(
    depNodes.map(async (depNode): Promise<undefined> => {
      const { files } = await depNode.fetching()

      depNode.requiresBuild = files.requiresBuild
      let sideEffectsCacheKey: string | undefined
      if (opts.sideEffectsCacheRead && files.sideEffectsMaps && !isEmpty(files.sideEffectsMaps)) {
        if (opts?.allowBuild?.(depNode.name, depNode.version) !== false) {
          sideEffectsCacheKey = calcDepState(opts.depGraph, opts.depsStateCache, depNode.depPath, {
            includeDepGraphHash: !opts.ignoreScripts && depNode.requiresBuild, // true when is built
            patchFileHash: depNode.patch?.file.hash,
          })
        }
      }
      const { importMethod, isBuilt } = await storeController.importPackage(depNode.dir, {
        disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
        filesResponse: files,
        force: opts.force,
        sideEffectsCacheKey,
        requiresBuild: depNode.patch != null || depNode.requiresBuild,
      })
      if (importMethod) {
        progressLogger.debug({
          method: importMethod,
          requester: opts.lockfileDir,
          status: 'imported',
          to: depNode.dir,
        })
      }
      depNode.isBuilt = isBuilt

      const selfDep = depNode.children[depNode.name]
      if (selfDep) {
        const pkg = opts.depGraph[selfDep]
        if (!pkg || !pkg.installable && pkg.optional) return
        const targetModulesDir = path.join(depNode.modules, depNode.name, 'node_modules')
        await limitLinking(async () => symlinkDependency(pkg.dir, targetModulesDir, depNode.name))
      }
    })
  )
}

async function linkAllModules (
  depNodes: DependenciesGraphNode[],
  depGraph: DependenciesGraph,
  opts: {
    lockfileDir: string
    optional: boolean
  }
): Promise<void> {
  await symlinkAllModules({
    deps: depNodes.map((depNode) => {
      const children = opts.optional
        ? depNode.children
        : pickBy((_, childAlias) => !depNode.optionalDependencies.has(childAlias), depNode.children)
      const childrenPaths: Record<string, string> = {}
      for (const [alias, childDepPath] of Object.entries(children ?? {})) {
        if (childDepPath.startsWith('link:')) {
          childrenPaths[alias] = path.resolve(opts.lockfileDir, childDepPath.slice(5))
        } else {
          const pkg = depGraph[childDepPath]
          if (!pkg || !pkg.installable && pkg.optional || alias === depNode.name) continue
          childrenPaths[alias] = pkg.dir
        }
      }
      return {
        children: childrenPaths,
        modules: depNode.modules,
        name: depNode.name,
      }
    }),
  })
}
