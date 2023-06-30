import { type Lockfile, type PatchFile } from '@pnpm/lockfile-types'
import { type PreferredVersions, type Resolution, type WorkspacePackages } from '@pnpm/resolver-base'
import { type StoreController } from '@pnpm/store-controller-types'
import {
  type AllowedDeprecatedVersions,
  type ProjectManifest,
  type ReadPackageHook,
  type Registries,
} from '@pnpm/types'
import partition from 'ramda/src/partition'
import zipObj from 'ramda/src/zipObj'
import { type WantedDependency } from './getNonDevWantedDependencies'
import {
  createNodeId,
  nodeIdContainsSequence,
} from './nodeIdUtils'
import {
  type ChildrenByParentDepPath,
  type DependenciesTree,
  type LinkedDependency,
  type ImporterToResolve,
  type ParentPkgAliases,
  type PendingNode,
  type PkgAddress,
  resolveRootDependencies,
  type ResolvedPackage,
  type ResolvedPackagesByDepPath,
} from './resolveDependencies'

export * from './nodeIdUtils'
export type { LinkedDependency, ResolvedPackage, DependenciesTree, DependenciesTreeNode } from './resolveDependencies'

export interface ResolvedDirectDependency {
  alias: string
  optional: boolean
  dev: boolean
  resolution: Resolution
  pkgId: string
  version: string
  name: string
  normalizedPref?: string
}

export interface Importer<T> {
  id: string
  manifest: ProjectManifest
  modulesDir: string
  removePackages?: string[]
  rootDir: string
  wantedDependencies: Array<T & WantedDependency>
}

export interface ImporterToResolveGeneric<T> extends Importer<T> {
  updatePackageManifest: boolean
  updateMatching?: (pkgName: string) => boolean
  hasRemovedDependencies?: boolean
  preferredVersions?: PreferredVersions
  wantedDependencies: Array<T & WantedDependency & { updateDepth: number }>
}

export interface ResolveDependenciesOptions {
  autoInstallPeers?: boolean
  allowBuild?: (pkgName: string) => boolean
  allowedDeprecatedVersions: AllowedDeprecatedVersions
  allowNonAppliedPatches: boolean
  currentLockfile: Lockfile
  dryRun: boolean
  engineStrict: boolean
  force: boolean
  forceFullResolution: boolean
  ignoreScripts?: boolean
  hooks: {
    readPackage?: ReadPackageHook
  }
  nodeVersion: string
  registries: Registries
  patchedDependencies?: Record<string, PatchFile>
  pnpmVersion: string
  preferredVersions?: PreferredVersions
  preferWorkspacePackages?: boolean
  resolutionMode?: 'highest' | 'time-based' | 'lowest-direct'
  resolvePeersFromWorkspaceRoot?: boolean
  linkWorkspacePackagesDepth?: number
  lockfileDir: string
  storeController: StoreController
  tag: string
  virtualStoreDir: string
  wantedLockfile: Lockfile
  workspacePackages: WorkspacePackages
}

export async function resolveDependencyTree<T> (
  importers: Array<ImporterToResolveGeneric<T>>,
  opts: ResolveDependenciesOptions
) {
  const wantedToBeSkippedPackageIds = new Set<string>()
  const ctx = {
    autoInstallPeers: opts.autoInstallPeers === true,
    allowBuild: opts.allowBuild,
    allowedDeprecatedVersions: opts.allowedDeprecatedVersions,
    childrenByParentDepPath: {} as ChildrenByParentDepPath,
    currentLockfile: opts.currentLockfile,
    defaultTag: opts.tag,
    dependenciesTree: new Map() as DependenciesTree<ResolvedPackage>,
    dryRun: opts.dryRun,
    engineStrict: opts.engineStrict,
    force: opts.force,
    forceFullResolution: opts.forceFullResolution,
    ignoreScripts: opts.ignoreScripts,
    linkWorkspacePackagesDepth: opts.linkWorkspacePackagesDepth ?? -1,
    lockfileDir: opts.lockfileDir,
    nodeVersion: opts.nodeVersion,
    outdatedDependencies: {} as { [pkgId: string]: string },
    patchedDependencies: opts.patchedDependencies,
    pendingNodes: [] as PendingNode[],
    pnpmVersion: opts.pnpmVersion,
    preferWorkspacePackages: opts.preferWorkspacePackages,
    readPackageHook: opts.hooks.readPackage,
    registries: opts.registries,
    resolvedPackagesByDepPath: {} as ResolvedPackagesByDepPath,
    resolutionMode: opts.resolutionMode,
    skipped: wantedToBeSkippedPackageIds,
    storeController: opts.storeController,
    virtualStoreDir: opts.virtualStoreDir,
    wantedLockfile: opts.wantedLockfile,
    appliedPatches: new Set<string>(),
    updatedSet: new Set<string>(),
    workspacePackages: opts.workspacePackages,
    missingPeersOfChildrenByPkgId: {},
  }

  const resolveArgs: ImporterToResolve[] = importers.map((importer) => {
    const projectSnapshot = opts.wantedLockfile.importers[importer.id]
    // This may be optimized.
    // We only need to proceed resolving every dependency
    // if the newly added dependency has peer dependencies.
    const proceed = importer.id === '.' || importer.hasRemovedDependencies === true || importer.wantedDependencies.some((wantedDep: any) => wantedDep.isNew) // eslint-disable-line @typescript-eslint/no-explicit-any
    const resolveOpts = {
      currentDepth: 0,
      parentPkg: {
        installable: true,
        nodeId: `>${importer.id}>`,
        optional: false,
        depPath: importer.id,
        rootDir: importer.rootDir,
      },
      proceed,
      resolvedDependencies: {
        ...projectSnapshot.dependencies,
        ...projectSnapshot.devDependencies,
        ...projectSnapshot.optionalDependencies,
      },
      updateDepth: -1,
      updateMatching: importer.updateMatching,
      prefix: importer.rootDir,
    }
    return {
      updatePackageManifest: importer.updatePackageManifest,
      parentPkgAliases: Object.fromEntries(
        importer.wantedDependencies.filter(({ alias }) => alias).map(({ alias }) => [alias, true])
      ) as ParentPkgAliases,
      preferredVersions: importer.preferredVersions ?? {},
      wantedDependencies: importer.wantedDependencies,
      options: resolveOpts,
    }
  })
  const { pkgAddressesByImporters, time } = await resolveRootDependencies(ctx, resolveArgs)
  const directDepsByImporterId = zipObj(importers.map(({ id }) => id), pkgAddressesByImporters)

  ctx.pendingNodes.forEach((pendingNode) => {
    ctx.dependenciesTree.set(pendingNode.nodeId, {
      children: () => buildTree(ctx, pendingNode.nodeId, pendingNode.resolvedPackage.id,
        ctx.childrenByParentDepPath[pendingNode.resolvedPackage.depPath], pendingNode.depth + 1, pendingNode.installable),
      depth: pendingNode.depth,
      installable: pendingNode.installable,
      resolvedPackage: pendingNode.resolvedPackage,
    })
  })

  const resolvedImporters = {} as {
    [id: string]: {
      directDependencies: ResolvedDirectDependency[]
      directNodeIdsByAlias: {
        [alias: string]: string
      }
      linkedDependencies: LinkedDependency[]
    }
  }

  for (const { id } of importers) {
    const directDeps = directDepsByImporterId[id]
    const [linkedDependencies, directNonLinkedDeps] = partition((dep) => dep.isLinkedDependency === true, directDeps) as [LinkedDependency[], PkgAddress[]]

    resolvedImporters[id] = {
      directDependencies: directDeps
        .map((dep) => {
          if (dep.isLinkedDependency === true) {
            return dep
          }
          const resolvedPackage = ctx.dependenciesTree.get(dep.nodeId)!.resolvedPackage as ResolvedPackage
          return {
            alias: dep.alias,
            dev: resolvedPackage.dev,
            name: resolvedPackage.name,
            normalizedPref: dep.normalizedPref,
            optional: resolvedPackage.optional,
            pkgId: resolvedPackage.id,
            resolution: resolvedPackage.resolution,
            version: resolvedPackage.version,
          }
        }),
      directNodeIdsByAlias: directNonLinkedDeps
        .reduce((acc, { alias, nodeId }) => {
          acc[alias] = nodeId
          return acc
        }, {} as Record<string, string>),
      linkedDependencies,
    }
  }

  return {
    dependenciesTree: ctx.dependenciesTree,
    outdatedDependencies: ctx.outdatedDependencies,
    resolvedImporters,
    resolvedPackagesByDepPath: ctx.resolvedPackagesByDepPath,
    wantedToBeSkippedPackageIds,
    appliedPatches: ctx.appliedPatches,
    time,
  }
}

function buildTree (
  ctx: {
    childrenByParentDepPath: ChildrenByParentDepPath
    dependenciesTree: DependenciesTree<ResolvedPackage>
    resolvedPackagesByDepPath: ResolvedPackagesByDepPath
    skipped: Set<string>
  },
  parentNodeId: string,
  parentId: string,
  children: Array<{ alias: string, depPath: string }>,
  depth: number,
  installable: boolean
) {
  const childrenNodeIds: Record<string, string> = {}
  for (const child of children) {
    if (child.depPath.startsWith('link:')) {
      childrenNodeIds[child.alias] = child.depPath
      continue
    }
    if (nodeIdContainsSequence(parentNodeId, parentId, child.depPath) || parentId === child.depPath) {
      continue
    }
    const childNodeId = createNodeId(parentNodeId, child.depPath)
    childrenNodeIds[child.alias] = childNodeId
    installable = installable && !ctx.skipped.has(child.depPath)
    ctx.dependenciesTree.set(childNodeId, {
      children: () => buildTree(ctx,
        childNodeId,
        child.depPath,
        ctx.childrenByParentDepPath[child.depPath],
        depth + 1,
        installable
      ),
      depth,
      installable,
      resolvedPackage: ctx.resolvedPackagesByDepPath[child.depPath],
    })
  }
  return childrenNodeIds
}
