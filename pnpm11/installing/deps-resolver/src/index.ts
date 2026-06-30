import path from 'node:path'

import type { Catalogs } from '@pnpm/catalogs.types'
import {
  packageManifestLogger,
} from '@pnpm/core-loggers'
import { findRuntimeNodeVersion, iterateHashedGraphNodes } from '@pnpm/deps.graph-hasher'
import { isRuntimeDepPath } from '@pnpm/deps.path'
import { PnpmError } from '@pnpm/error'
import type {
  LockfileObject,
  ProjectSnapshot,
} from '@pnpm/lockfile.types'
import { verifyPatches } from '@pnpm/patching.config'
import { safeReadPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import {
  getAllDependenciesFromManifest,
  getSpecFromPackageManifest,
} from '@pnpm/pkg-manifest.utils'
import type { ResolutionPolicyViolation } from '@pnpm/resolving.resolver-base'
import {
  type AllowBuild,
  DEPENDENCIES_FIELDS,
  type DependenciesField,
  type DependencyManifest,
  type DepPath,
  type PeerDependencyIssuesByProjects,
  type PinnedVersion,
  type PkgIdWithPatchHash,
  type ProjectId,
  type ProjectManifest,
  type ProjectRootDir,
  type SupportedArchitectures,
} from '@pnpm/types'
import { isSubdir } from 'is-subdir'
import { difference, zipWith } from 'ramda'

import { depPathToRef } from './depPathToRef.js'
import { getCatalogSnapshots } from './getCatalogSnapshots.js'
import { getWantedDependencies, type WantedDependency } from './getWantedDependencies.js'
import type { NodeId } from './nextNodeId.js'
import { createNodeIdForLinkedLocalPkg, type DependenciesTree, type UpdateMatchingFunction } from './resolveDependencies.js'
import {
  type Importer,
  type LinkedDependency,
  type ResolvedDirectDependency,
  type ResolveDependenciesOptions,
  resolveDependencyTree,
  type ResolvedPackage,
} from './resolveDependencyTree.js'
import {
  type DependenciesByProjectId,
  type GenericDependenciesGraphNodeWithResolvedChildren,
  type GenericDependenciesGraphWithResolvedChildren,
  resolvePeers,
} from './resolvePeers.js'
import { toResolveImporter } from './toResolveImporter.js'
import { updateLockfile } from './updateLockfile.js'
import { updateProjectManifest } from './updateProjectManifest.js'

export type DependenciesGraph = GenericDependenciesGraphWithResolvedChildren<ResolvedPackage>

export type DependenciesGraphNode = GenericDependenciesGraphNodeWithResolvedChildren & ResolvedPackage

export {
  getWantedDependencies,
  type LinkedDependency,
  type PinnedVersion,
  type ResolvedPackage,
  type UpdateMatchingFunction,
  type WantedDependency,
}
export { assertValidDependencyAliases, isValidDependencyAlias } from './validateDependencyAlias.js'

interface ProjectToLink {
  binsDir: string
  declaredDirectDependencies: Set<string>
  directNodeIdsByAlias: Map<string, NodeId>
  explicitlyRequestedDirectDependencies: Set<string>
  id: ProjectId
  linkedDependencies: LinkedDependency[]
  manifest: ProjectManifest
  modulesDir: string
  rootDir: ProjectRootDir
  topParents: Array<{ name: string, version: string }>
}

export interface ImporterToResolve extends Importer<{
  isNew?: boolean
  nodeExecPath?: string
  pinnedVersion?: PinnedVersion
  updateSpec?: boolean
  preserveNonSemverVersionSpec?: boolean
}> {
  peer?: boolean
  pinnedVersion?: PinnedVersion
  binsDir: string
  manifest: ProjectManifest
  originalManifest?: ProjectManifest
  update?: boolean
  updateMatching?: UpdateMatchingFunction
  updatePackageManifest: boolean
  targetDependenciesField?: DependenciesField
}

export interface ResolveDependenciesResult {
  dependenciesByProjectId: DependenciesByProjectId
  dependenciesGraph: GenericDependenciesGraphWithResolvedChildren<ResolvedPackage>
  updatedCatalogs?: Catalogs | undefined
  outdatedDependencies: {
    [pkgId: string]: string
  }
  linkedDependenciesByProjectId: Record<string, LinkedDependency[]>
  newLockfile: LockfileObject
  peerDependencyIssuesByProjects: PeerDependencyIssuesByProjects
  waitTillAllFetchingsFinish: () => Promise<void>
  wantedToBeSkippedPackageIds: Set<string>
  /**
   * Policy violations collected inline during resolution — each
   * resolver pushes to the list whenever it picks a version that
   * trips one of its own checks (today: `minimumReleaseAge`). The
   * install command reacts via `handleResolutionPolicyViolations`
   * (prompt / abort) and `mutateModules` forwards the array out so
   * the auto-persist path at the install's tail can drain it into
   * the workspace manifest. Empty when no policy is active or no
   * pick violates.
   */
  resolutionPolicyViolations: ResolutionPolicyViolation[]
}

export async function resolveDependencies (
  importers: ImporterToResolve[],
  opts: ResolveDependenciesOptions & {
    defaultUpdateDepth: number
    dedupePeerDependents?: boolean
    dedupePeers?: boolean
    dedupeDirectDeps?: boolean
    dedupeInjectedDeps?: boolean
    excludeLinksFromLockfile?: boolean
    preserveWorkspaceProtocol: boolean
    saveWorkspaceProtocol: 'rolling' | boolean
    lockfileIncludeTarballUrl?: boolean
    allowUnusedPatches?: boolean
    enableGlobalVirtualStore?: boolean
    allProjectIds: string[]
    /**
     * Generic checkpoint invoked between `resolveDependencyTree` and
     * `resolvePeers` once any inline-collected policy violations have
     * been gathered. Callers can prompt, persist, or throw based on
     * the violations. Throwing unwinds before any peer-dep work,
     * lockfile write, package.json update, or modules-dir change.
     * Intentionally policy-neutral: each resolver owns its violation
     * codes and the hook implementer (install command) decides what
     * to do with them.
     */
    handleResolutionPolicyViolations?: (violations: readonly ResolutionPolicyViolation[]) => Promise<void>
  }
): Promise<ResolveDependenciesResult> {
  const _toResolveImporter = toResolveImporter.bind(null, {
    defaultUpdateDepth: opts.defaultUpdateDepth,
    lockfileOnly: opts.dryRun,
    preferredVersions: opts.preferredVersions,
    virtualStoreDir: opts.virtualStoreDir,
    globalVirtualStoreDir: opts.globalVirtualStoreDir,
    workspacePackages: opts.workspacePackages,
    noDependencySelectors: importers.every(({ wantedDependencies }) => wantedDependencies.length === 0),
  })
  const projectsToResolve = await Promise.all(importers.map(async (project) => _toResolveImporter(project)))
  const {
    dependenciesTree,
    outdatedDependencies,
    resolvedImporters,
    resolvedPkgsById,
    wantedToBeSkippedPackageIds,
    appliedPatches,
    time,
    allPeerDepNames,
    resolutionPolicyViolations,
  } = await resolveDependencyTree(projectsToResolve, opts)

  // Resolver-policy gate between main resolution and peer-dep
  // resolution: every resolver records its own policy violations
  // inline as it picks each version, and we hand the accumulated
  // list to the install command's hook. The hook throws to abort
  // cleanly — nothing on disk has changed yet, and we haven't paid
  // the cost of peer resolution. Dispatch stays policy-neutral: each
  // resolver owns its violation codes, and the hook implementer
  // decides what to do with them.
  //
  // If violations fired but no hook was wired, throw rather than
  // silently dropping them — the resolver-policy contract is "every
  // pick that trips a check produces a violation that gets handled";
  // a missing handler means the caller forgot to opt in and would
  // otherwise see policy-rejected versions land in the lockfile.
  if (resolutionPolicyViolations.length > 0) {
    if (!opts.handleResolutionPolicyViolations) {
      throw new PnpmError(
        'RESOLUTION_POLICY_VIOLATIONS_UNHANDLED',
        `${resolutionPolicyViolations.length} resolution-policy ${resolutionPolicyViolations.length === 1 ? 'violation was' : 'violations were'} produced but no handleResolutionPolicyViolations callback was wired to react to them.`,
        {
          hint: 'Internal: resolveDependencies needs a handleResolutionPolicyViolations callback whenever a policy that can produce violations (today: minimumReleaseAge) is active. Wire setupPolicyHandlers (in @pnpm/installing.commands) or supply a callback directly.',
        }
      )
    }
    await opts.handleResolutionPolicyViolations(resolutionPolicyViolations)
  }

  opts.storeController.clearResolutionCache()

  // We only check whether patches were applied in cases when the whole lockfile was reanalyzed.
  if (
    opts.patchedDependencies &&
    (opts.forceFullResolution || !Object.keys(opts.wantedLockfile.packages ?? {})?.length) &&
    Object.keys(opts.wantedLockfile.importers).length === importers.length
  ) {
    verifyPatches({
      patchedDependencies: opts.patchedDependencies,
      appliedPatches,
      allowUnusedPatches: opts.allowUnusedPatches,
    })
  }

  const projectsToLink = await Promise.all<ProjectToLink>(projectsToResolve.map(async (project) => {
    const resolvedImporter = resolvedImporters[project.id]

    const topParents: Array<{ name: string, version: string, alias?: string, linkedDir?: string }> = project.manifest
      ? await getTopParents(
        difference(
          Object.keys(getAllDependenciesFromManifest(project.manifest)),
          resolvedImporter.directDependencies.map(({ alias }) => alias) || []
        ),
        project.modulesDir
      )
      : []
    for (const linkedDependency of resolvedImporter.linkedDependencies) {
      // The location of the external link may vary on different machines, so it is better not to include it in the lockfile.
      // As a workaround, we symlink to the root of node_modules, which is a symlink to the actual location of the external link.
      const target = !opts.excludeLinksFromLockfile || isSubdir(opts.lockfileDir, linkedDependency.resolution.directory)
        ? linkedDependency.resolution.directory
        : path.join(project.modulesDir, linkedDependency.alias)
      const linkedDir = createNodeIdForLinkedLocalPkg(opts.lockfileDir, target) as string
      topParents.push({
        name: linkedDependency.alias,
        version: linkedDependency.version,
        linkedDir,
      })
    }

    return {
      binsDir: project.binsDir,
      declaredDirectDependencies: new Set([
        ...Object.keys(project.manifest == null ? {} : getAllDependenciesFromManifest(project.manifest)),
        ...project.wantedDependencies.flatMap(({ alias, isNew }) => isNew && alias != null ? [alias] : []),
      ]),
      directNodeIdsByAlias: resolvedImporter.directNodeIdsByAlias,
      explicitlyRequestedDirectDependencies: new Set(
        project.wantedDependencies.flatMap(({ alias, bareSpecifier, isNew, prevSpecifier, updateSpec }) =>
          alias != null && (isNew === true || updateSpec === true || (prevSpecifier != null && bareSpecifier !== prevSpecifier))
            ? [alias]
            : []
        )
      ),
      id: project.id,
      linkedDependencies: resolvedImporter.linkedDependencies,
      manifest: project.manifest,
      modulesDir: project.modulesDir,
      rootDir: project.rootDir,
      topParents,
    }
  }))

  const peerResolutionOpts = {
    allPeerDepNames,
    dependenciesTree,
    dedupePeerDependents: opts.dedupePeerDependents,
    dedupePeers: opts.dedupePeers,
    dedupeInjectedDeps: opts.dedupeInjectedDeps,
    lockfileDir: opts.lockfileDir,
    projects: projectsToLink,
    virtualStoreDir: opts.virtualStoreDir,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    resolvePeersFromWorkspaceRoot: Boolean(opts.resolvePeersFromWorkspaceRoot),
    resolvedImporters,
    peersSuffixMaxLength: opts.peersSuffixMaxLength,
    workspaceProjectIds: new Set([...opts.allProjectIds, ...Object.keys(opts.wantedLockfile.importers)]),
  }
  const initiallyResolvedPeers = await resolvePeers(peerResolutionOpts)
  // A second pass reuses the peer contexts already recorded in the lockfile so a
  // writable install does not rewrite dependency instances whose locked provider
  // is still valid and present. It can only differ from the first pass for nodes
  // that carry a locked peer context, so it is skipped when none do (e.g. a fresh
  // install) to avoid resolving peers twice for no benefit.
  const {
    dependenciesGraph,
    dependenciesByProjectId,
    peerDependencyIssuesByProjects,
  } = treeHasLockedPeerContexts(dependenciesTree)
    ? await resolvePeers({
      ...peerResolutionOpts,
      resolvedPeerProviderPaths: initiallyResolvedPeers.pathsByNodeId,
    })
    : initiallyResolvedPeers

  const linkedDependenciesByProjectId: Record<string, LinkedDependency[]> = {}
  await Promise.all(projectsToResolve.map(async (project, index) => {
    const resolvedImporter = resolvedImporters[project.id]
    linkedDependenciesByProjectId[project.id] = resolvedImporter.linkedDependencies
    let updatedManifest: ProjectManifest | undefined
    let updatedOriginalManifest: ProjectManifest | undefined
    if (project.updatePackageManifest) {
      [updatedManifest, updatedOriginalManifest] = await updateProjectManifest(project, {
        directDependencies: resolvedImporter.directDependencies,
        preserveWorkspaceProtocol: opts.preserveWorkspaceProtocol,
        saveWorkspaceProtocol: opts.saveWorkspaceProtocol,
      })
    } else {
      updatedManifest = project.manifest
      updatedOriginalManifest = project.originalManifest
      packageManifestLogger.debug({
        prefix: project.rootDir,
        updated: project.manifest,
      })
    }

    if (updatedManifest != null) {
      if (opts.autoInstallPeers) {
        if (updatedManifest.peerDependencies) {
          const allDeps = getAllDependenciesFromManifest(updatedManifest)
          for (const [peerName, peerRange] of Object.entries(updatedManifest.peerDependencies)) {
            if (allDeps[peerName]) continue
            updatedManifest.dependencies ??= {}
            updatedManifest.dependencies[peerName] = peerRange
          }
        }
      }
      const projectSnapshot = opts.wantedLockfile.importers[project.id]
      opts.wantedLockfile.importers[project.id] = addDirectDependenciesToLockfile(
        updatedManifest,
        projectSnapshot,
        resolvedImporter.linkedDependencies,
        resolvedImporter.directDependencies,
        opts.excludeLinksFromLockfile
      )
    }

    importers[index].manifest = updatedOriginalManifest ?? project.originalManifest ?? project.manifest

    for (const [alias, depPath] of dependenciesByProjectId[project.id].entries()) {
      const projectSnapshot = opts.wantedLockfile.importers[project.id]
      if (project.manifest.dependenciesMeta != null) {
        projectSnapshot.dependenciesMeta = project.manifest.dependenciesMeta
      }

      const depNode = dependenciesGraph[depPath]

      const ref = depPathToRef(depPath, {
        alias,
        realName: depNode.name,
      })
      if (projectSnapshot.dependencies?.[alias]) {
        projectSnapshot.dependencies[alias] = ref
      } else if (projectSnapshot.devDependencies?.[alias]) {
        projectSnapshot.devDependencies[alias] = ref
      } else if (projectSnapshot.optionalDependencies?.[alias]) {
        projectSnapshot.optionalDependencies[alias] = ref
      }
    }
  }))

  let updatedCatalogs: Record<string, Record<string, string>> | undefined
  for (const project of projectsToResolve) {
    if (!project.updatePackageManifest) continue
    const resolvedImporter = resolvedImporters[project.id]
    for (let i = 0; i < resolvedImporter.directDependencies.length; i++) {
      const updateSpec = project.wantedDependencies[i]?.updateSpec ?? false
      if (!updateSpec) continue
      const dep = resolvedImporter.directDependencies[i]
      if (dep.catalogLookup == null) continue
      // If normalizedBareSpecifier isn't defined, this catalog entry was resolved from cache.
      // Avoid updating the updatedCatalogs map since it is likely unchanged.
      if (dep.normalizedBareSpecifier == null) continue
      updatedCatalogs ??= {}
      updatedCatalogs[dep.catalogLookup.catalogName] ??= {}
      updatedCatalogs[dep.catalogLookup.catalogName][dep.alias] = dep.normalizedBareSpecifier
    }
  }

  if (opts.dedupeDirectDeps) {
    const rootDeps = dependenciesByProjectId['.']
    if (rootDeps) {
      for (const [id, deps] of Object.entries(dependenciesByProjectId)) {
        if (id === '.') continue
        for (const [alias, depPath] of deps.entries()) {
          if (depPath === rootDeps.get(alias)) {
            deps.delete(alias)
          }
        }
      }
    }
  }

  await waitForResolutionFetches(resolvedPkgsById)

  const newLockfile = updateLockfile({
    dependenciesGraph,
    lockfile: opts.wantedLockfile,
    prefix: opts.virtualStoreDir,
    registries: opts.registries,
    lockfileIncludeTarballUrl: opts.lockfileIncludeTarballUrl,
  })
  if (time) {
    newLockfile.time = {
      ...opts.wantedLockfile.time,
      ...time,
    }
  }

  newLockfile.catalogs = getCatalogSnapshots(
    Object.values(resolvedImporters).flatMap(({ directDependencies }) => directDependencies),
    updatedCatalogs)

  // waiting till package requests are finished
  async function waitTillAllFetchingsFinish (): Promise<void> {
    await Promise.all(Object.values(resolvedPkgsById).map(async ({ fetching }) => {
      try {
        await fetching?.()
      } catch {}
    }))
  }

  return {
    dependenciesByProjectId,
    dependenciesGraph: extendGraph(dependenciesGraph, opts),
    outdatedDependencies,
    linkedDependenciesByProjectId,
    updatedCatalogs,
    newLockfile,
    peerDependencyIssuesByProjects,
    waitTillAllFetchingsFinish,
    wantedToBeSkippedPackageIds,
    resolutionPolicyViolations,
  }
}

function treeHasLockedPeerContexts (dependenciesTree: DependenciesTree<ResolvedPackage>): boolean {
  for (const node of dependenciesTree.values()) {
    if (node.lockedPeerContext != null) return true
  }
  return false
}

function addDirectDependenciesToLockfile (
  newManifest: ProjectManifest,
  projectSnapshot: ProjectSnapshot,
  linkedPackages: Array<{ alias: string }>,
  directDependencies: ResolvedDirectDependency[],
  excludeLinksFromLockfile?: boolean
): ProjectSnapshot {
  const newProjectSnapshot: ProjectSnapshot & Required<Pick<ProjectSnapshot, 'dependencies' | 'devDependencies' | 'optionalDependencies'>> = {
    dependencies: {},
    devDependencies: {},
    optionalDependencies: {},
    specifiers: {},
  }

  if (newManifest.publishConfig?.directory) {
    newProjectSnapshot.publishDirectory = newManifest.publishConfig.directory
  }

  for (const linkedPkg of linkedPackages) {
    newProjectSnapshot.specifiers[linkedPkg.alias] = getSpecFromPackageManifest(newManifest, linkedPkg.alias)
  }

  const directDependenciesByAlias: Record<string, ResolvedDirectDependency> = {}
  for (const directDependency of directDependencies) {
    directDependenciesByAlias[directDependency.alias] = directDependency
  }

  const allDeps = Array.from(new Set(Object.keys(getAllDependenciesFromManifest(newManifest))))

  for (const alias of allDeps) {
    const dep = directDependenciesByAlias[alias]
    const spec = dep && getSpecFromPackageManifest(newManifest, dep.alias)
    if (
      dep &&
      (
        !excludeLinksFromLockfile ||
        !(dep as LinkedDependency).isLinkedDependency ||
        spec.startsWith('workspace:')
      )
    ) {
      const ref = depPathToRef(dep.pkgId, {
        alias: dep.alias,
        realName: dep.name,
      })
      if (dep.dev) {
        newProjectSnapshot.devDependencies[dep.alias] = ref
      } else if (dep.optional) {
        newProjectSnapshot.optionalDependencies[dep.alias] = ref
      } else {
        newProjectSnapshot.dependencies[dep.alias] = ref
      }
      newProjectSnapshot.specifiers[dep.alias] = spec
    } else if (projectSnapshot.specifiers[alias]) {
      newProjectSnapshot.specifiers[alias] = projectSnapshot.specifiers[alias]
      if (projectSnapshot.dependencies?.[alias]) {
        newProjectSnapshot.dependencies[alias] = projectSnapshot.dependencies[alias]
      } else if (projectSnapshot.optionalDependencies?.[alias]) {
        newProjectSnapshot.optionalDependencies[alias] = projectSnapshot.optionalDependencies[alias]
      } else if (projectSnapshot.devDependencies?.[alias]) {
        newProjectSnapshot.devDependencies[alias] = projectSnapshot.devDependencies[alias]
      }
    }
  }

  alignDependencyTypes(newManifest, newProjectSnapshot)

  return newProjectSnapshot
}

function alignDependencyTypes (manifest: ProjectManifest, projectSnapshot: ProjectSnapshot): void {
  const depTypesOfAliases = getAliasToDependencyTypeMap(manifest)

  // Aligning the dependency types in pnpm-lock.yaml
  for (const depType of DEPENDENCIES_FIELDS) {
    if (projectSnapshot[depType] == null) continue
    for (const [alias, ref] of Object.entries(projectSnapshot[depType] ?? {})) {
      if (depType === depTypesOfAliases[alias] || !depTypesOfAliases[alias]) continue
      projectSnapshot[depTypesOfAliases[alias]]![alias] = ref
      delete projectSnapshot[depType]![alias]
    }
  }
}

/**
 * Waits for fetches that complete resolution data used by the lockfile snapshot and
 * virtual-store paths. Other package fetches are awaited later by `waitTillAllFetchingsFinish`.
 */
async function waitForResolutionFetches (resolvedPkgsById: Record<string, ResolvedPackage>): Promise<void> {
  const fetches: Array<Promise<unknown>> = []
  for (const pkg of Object.values(resolvedPkgsById)) {
    if (pkg.resolutionNeedsFetch && pkg.fetching != null) {
      fetches.push(pkg.fetching())
    }
  }
  if (fetches.length > 0) {
    await Promise.all(fetches)
  }
}

function getAliasToDependencyTypeMap (manifest: ProjectManifest): Record<string, DependenciesField> {
  const depTypesOfAliases: Record<string, DependenciesField> = {}
  for (const depType of DEPENDENCIES_FIELDS) {
    if (manifest[depType] == null) continue
    for (const alias of Object.keys(manifest[depType] ?? {})) {
      if (!depTypesOfAliases[alias]) {
        depTypesOfAliases[alias] = depType
      }
    }
  }
  return depTypesOfAliases
}

async function getTopParents (pkgAliases: string[], modulesDir: string): Promise<DependencyManifest[]> {
  const pkgs = await Promise.all(
    pkgAliases.map((alias) => path.join(modulesDir, alias)).map(safeReadPackageJsonFromDir)
  )
  return zipWith((manifest, alias) => {
    if (!manifest) return null
    return {
      alias,
      name: manifest.name,
      version: manifest.version,
    }
  }, pkgs, pkgAliases)
    .filter(Boolean) as DependencyManifest[]
}

function * iterateGraphPkgMetaEntries (graph: DependenciesGraph, runtimeOnly?: boolean): IterableIterator<{ depPath: DepPath; name: string; version: string; pkgIdWithPatchHash: PkgIdWithPatchHash }> {
  for (const depPath in graph) {
    if (Object.hasOwn(graph, depPath)) {
      if (runtimeOnly && !isRuntimeDepPath(depPath as DepPath)) continue
      const { name, version, pkgIdWithPatchHash } = graph[depPath as DepPath]
      yield { depPath: depPath as DepPath, name, version, pkgIdWithPatchHash }
    }
  }
}

function extendGraph (
  graph: DependenciesGraph,
  opts: {
    allowBuild?: AllowBuild
    globalVirtualStoreDir: string
    enableGlobalVirtualStore?: boolean
    supportedArchitectures?: SupportedArchitectures
  }
): DependenciesGraph {
  const pkgMetaIter = iterateGraphPkgMetaEntries(graph, !opts.enableGlobalVirtualStore)
  // Only use allowBuild for engine-agnostic hash optimization when GVS is on
  const allowBuild = opts.enableGlobalVirtualStore ? opts.allowBuild : undefined
  // Anchor every snapshot's engine hash to the project-pinned Node
  // version (from `engines.runtime` / `devEngines.runtime`) when the
  // resolver produced one — the graph carries it as a
  // `node@runtime:<version>` key. Without this, GVS slots for
  // approved-build packages would hash under the runner's
  // `process.version` instead of the script-runner Node, splitting
  // the cache between pinned and non-pinned installs on the same host.
  const nodeVersion = findRuntimeNodeVersion(Object.keys(graph))
  for (const { pkgMeta: { depPath }, hash } of iterateHashedGraphNodes(graph, pkgMetaIter, allowBuild, opts.supportedArchitectures, nodeVersion)) {
    const modules = path.join(opts.globalVirtualStoreDir, hash, 'node_modules')
    const node = graph[depPath]
    Object.assign(node, {
      modules,
      dir: path.join(modules, node.name),
    })
  }
  return graph
}
