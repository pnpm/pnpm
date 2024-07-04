import { resolveFromCatalog } from '@pnpm/catalogs.resolver'
import { type Catalogs } from '@pnpm/catalogs.types'
import { type Lockfile, type PatchFile } from '@pnpm/lockfile-types'
import { type PreferredVersions, type Resolution, type WorkspacePackages } from '@pnpm/resolver-base'
import { type StoreController } from '@pnpm/store-controller-types'
import {
  type SupportedArchitectures,
  type AllowedDeprecatedVersions,
  type PkgResolutionId,
  type ProjectManifest,
  type ProjectId,
  type ReadPackageHook,
  type Registries,
  type ProjectRootDir,
} from '@pnpm/types'
import partition from 'ramda/src/partition'
import zipObj from 'ramda/src/zipObj'
import { type WantedDependency } from './getNonDevWantedDependencies'
import { type NodeId, nextNodeId } from './nextNodeId'
import { parentIdsContainSequence } from './parentIdsContainSequence'
import {
  type ChildrenByParentId,
  type DependenciesTree,
  type LinkedDependency,
  type ImporterToResolve,
  type ImporterToResolveOptions,
  type ParentPkgAliases,
  type PendingNode,
  type PkgAddress,
  resolveRootDependencies,
  type ResolvedPackage,
  type ResolvedPkgsById,
  type ResolutionContext,
} from './resolveDependencies'

export type { LinkedDependency, ResolvedPackage, DependenciesTree, DependenciesTreeNode } from './resolveDependencies'

export interface ResolvedImporters {
  [id: string]: {
    directDependencies: ResolvedDirectDependency[]
    directNodeIdsByAlias: Map<string, NodeId>
    linkedDependencies: LinkedDependency[]
  }
}

export interface ResolvedDirectDependency {
  alias: string
  optional: boolean
  dev: boolean
  resolution: Resolution
  pkgId: PkgResolutionId
  version: string
  name: string
  normalizedPref?: string
  catalogLookup?: CatalogLookupMetadata
}

/**
 * Information related to the catalog entry for this dependency if it was
 * requested through the catalog protocol.
 */
export interface CatalogLookupMetadata {
  readonly catalogName: string
  readonly specifier: string
}

export interface Importer<WantedDepExtraProps> {
  id: ProjectId
  manifest: ProjectManifest
  modulesDir: string
  removePackages?: string[]
  rootDir: ProjectRootDir
  wantedDependencies: Array<WantedDepExtraProps & WantedDependency>
}

export interface ImporterToResolveGeneric<WantedDepExtraProps> extends Importer<WantedDepExtraProps> {
  updatePackageManifest: boolean
  updateMatching?: (pkgName: string) => boolean
  hasRemovedDependencies?: boolean
  preferredVersions?: PreferredVersions
  wantedDependencies: Array<WantedDepExtraProps & WantedDependency & { updateDepth: number }>
}

export interface ResolveDependenciesOptions {
  autoInstallPeers?: boolean
  autoInstallPeersFromHighestMatch?: boolean
  allowBuild?: (pkgName: string) => boolean
  allowedDeprecatedVersions: AllowedDeprecatedVersions
  allowNonAppliedPatches: boolean
  catalogs?: Catalogs
  currentLockfile: Lockfile
  dedupePeerDependents?: boolean
  dryRun: boolean
  engineStrict: boolean
  force: boolean
  forceFullResolution: boolean
  ignoreScripts?: boolean
  hooks: {
    readPackage?: ReadPackageHook
  }
  nodeVersion?: string
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
  virtualStoreDirMaxLength: number
  wantedLockfile: Lockfile
  workspacePackages: WorkspacePackages
  supportedArchitectures?: SupportedArchitectures
  updateToLatest?: boolean
  peersSuffixMaxLength: number
}

export interface ResolveDependencyTreeResult {
  allPeerDepNames: Set<string>
  dependenciesTree: DependenciesTree<ResolvedPackage>
  outdatedDependencies: {
    [pkgId: string]: string
  }
  resolvedImporters: ResolvedImporters
  resolvedPkgsById: ResolvedPkgsById
  wantedToBeSkippedPackageIds: Set<string>
  appliedPatches: Set<string>
  time?: Record<string, string>
}

export async function resolveDependencyTree<T> (
  importers: Array<ImporterToResolveGeneric<T>>,
  opts: ResolveDependenciesOptions
): Promise<ResolveDependencyTreeResult> {
  const wantedToBeSkippedPackageIds = new Set<PkgResolutionId>()
  const autoInstallPeers = opts.autoInstallPeers === true
  const ctx: ResolutionContext = {
    autoInstallPeers,
    autoInstallPeersFromHighestMatch: opts.autoInstallPeersFromHighestMatch === true,
    allowBuild: opts.allowBuild,
    allowedDeprecatedVersions: opts.allowedDeprecatedVersions,
    catalogResolver: resolveFromCatalog.bind(null, opts.catalogs ?? {}),
    childrenByParentId: {} as ChildrenByParentId,
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
    resolvedPkgsById: {} as ResolvedPkgsById,
    resolutionMode: opts.resolutionMode,
    skipped: wantedToBeSkippedPackageIds,
    storeController: opts.storeController,
    virtualStoreDir: opts.virtualStoreDir,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    wantedLockfile: opts.wantedLockfile,
    appliedPatches: new Set<string>(),
    updatedSet: new Set<string>(),
    workspacePackages: opts.workspacePackages,
    missingPeersOfChildrenByPkgId: {},
    hoistPeers: autoInstallPeers || opts.dedupePeerDependents,
    allPeerDepNames: new Set(),
  }

  const resolveArgs: ImporterToResolve[] = importers.map((importer) => {
    const projectSnapshot = opts.wantedLockfile.importers[importer.id]
    // This may be optimized.
    // We only need to proceed resolving every dependency
    // if the newly added dependency has peer dependencies.
    const proceed = importer.id === '.' || importer.hasRemovedDependencies === true || importer.wantedDependencies.some((wantedDep: any) => wantedDep.isNew) // eslint-disable-line @typescript-eslint/no-explicit-any
    const resolveOpts: ImporterToResolveOptions = {
      currentDepth: 0,
      parentPkg: {
        installable: true,
        nodeId: importer.id as unknown as NodeId,
        optional: false,
        pkgId: importer.id as unknown as PkgResolutionId,
        rootDir: importer.rootDir,
      },
      parentIds: [importer.id as unknown as PkgResolutionId],
      proceed,
      resolvedDependencies: {
        ...projectSnapshot.dependencies,
        ...projectSnapshot.devDependencies,
        ...projectSnapshot.optionalDependencies,
      },
      updateDepth: -1,
      updateMatching: importer.updateMatching,
      prefix: importer.rootDir,
      supportedArchitectures: opts.supportedArchitectures,
      updateToLatest: opts.updateToLatest,
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
      children: () => buildTree(ctx, pendingNode.resolvedPackage.id,
        pendingNode.parentIds,
        ctx.childrenByParentId[pendingNode.resolvedPackage.id], pendingNode.depth + 1, pendingNode.installable),
      depth: pendingNode.depth,
      installable: pendingNode.installable,
      resolvedPackage: pendingNode.resolvedPackage,
    })
  })

  const resolvedImporters: ResolvedImporters = {}

  for (const { id, wantedDependencies } of importers) {
    const directDeps = dedupeSameAliasDirectDeps(directDepsByImporterId[id], wantedDependencies)
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
            catalogLookup: dep.catalogLookup,
            dev: resolvedPackage.dev,
            name: resolvedPackage.name,
            normalizedPref: dep.normalizedPref,
            optional: resolvedPackage.optional,
            pkgId: resolvedPackage.id,
            resolution: resolvedPackage.resolution,
            version: resolvedPackage.version,
          }
        }),
      directNodeIdsByAlias: new Map(directNonLinkedDeps.map(({ alias, nodeId }) => [alias, nodeId])),
      linkedDependencies,
    }
  }

  return {
    dependenciesTree: ctx.dependenciesTree,
    outdatedDependencies: ctx.outdatedDependencies,
    resolvedImporters,
    resolvedPkgsById: ctx.resolvedPkgsById,
    wantedToBeSkippedPackageIds,
    appliedPatches: ctx.appliedPatches,
    time,
    allPeerDepNames: ctx.allPeerDepNames,
  }
}

function buildTree (
  ctx: {
    childrenByParentId: ChildrenByParentId
    dependenciesTree: DependenciesTree<ResolvedPackage>
    resolvedPkgsById: ResolvedPkgsById
    skipped: Set<PkgResolutionId>
  },
  parentId: PkgResolutionId,
  parentIds: PkgResolutionId[],
  children: Array<{ alias: string, id: PkgResolutionId }>,
  depth: number,
  installable: boolean
): Record<string, NodeId> {
  const childrenNodeIds: Record<string, NodeId> = {}
  for (const child of children) {
    if (child.id.startsWith('link:')) {
      childrenNodeIds[child.alias] = child.id as unknown as NodeId
      continue
    }
    if (parentIdsContainSequence(parentIds, parentId, child.id) || parentId === child.id) {
      continue
    }
    const childNodeId = nextNodeId()
    childrenNodeIds[child.alias] = childNodeId
    installable = installable || !ctx.skipped.has(child.id)
    ctx.dependenciesTree.set(childNodeId, {
      children: () => buildTree(ctx,
        child.id,
        [...parentIds, child.id],
        ctx.childrenByParentId[child.id],
        depth + 1,
        installable
      ),
      depth,
      installable,
      resolvedPackage: ctx.resolvedPkgsById[child.id],
    })
  }
  return childrenNodeIds
}

/**
  * There may be cases where multiple dependencies have the same alias in the directDeps array.
  * E.g., when there is "is-negative: github:kevva/is-negative#1.0.0" in the package.json dependencies,
  * and then re-execute `pnpm add github:kevva/is-negative#1.0.1`.
  * In order to make sure that the latest 1.0.1 version is installed, we need to remove the duplicate dependency.
  * fix https://github.com/pnpm/pnpm/issues/6966
  */
function dedupeSameAliasDirectDeps (directDeps: Array<PkgAddress | LinkedDependency>, wantedDependencies: Array<WantedDependency & { isNew?: boolean }>): Array<PkgAddress | LinkedDependency> {
  const deps = new Map<string, PkgAddress | LinkedDependency>()
  for (const directDep of directDeps) {
    const { alias, normalizedPref } = directDep
    if (!deps.has(alias)) {
      deps.set(alias, directDep)
    } else {
      const wantedDep = wantedDependencies.find(dep =>
        dep.alias ? dep.alias === alias : dep.pref === normalizedPref
      )
      if (wantedDep?.isNew) {
        deps.set(alias, directDep)
      }
    }
  }
  return Array.from(deps.values())
}
