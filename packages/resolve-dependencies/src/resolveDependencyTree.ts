import { Lockfile, PatchFile } from '@pnpm/lockfile-types'
import { PreferredVersions, Resolution, WorkspacePackages } from '@pnpm/resolver-base'
import { StoreController } from '@pnpm/store-controller-types'
import {
  AllowedDeprecatedVersions,
  ProjectManifest,
  ReadPackageHook,
  Registries,
} from '@pnpm/types'
import fromPairs from 'ramda/src/fromPairs'
import partition from 'ramda/src/partition'
import zipObj from 'ramda/src/zipObj'
import { WantedDependency } from './getNonDevWantedDependencies'
import {
  createNodeId,
  nodeIdContainsSequence,
} from './nodeIdUtils'
import {
  ChildrenByParentDepPath,
  DependenciesTree,
  LinkedDependency,
  ImporterToResolve,
  ParentPkgAliases,
  PendingNode,
  PkgAddress,
  resolveRootDependencies,
  ResolvedPackage,
  ResolvedPackagesByDepPath,
} from './resolveDependencies'

export * from './nodeIdUtils'
export { LinkedDependency, ResolvedPackage, DependenciesTree, DependenciesTreeNode } from './resolveDependencies'

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
  hasRemovedDependencies?: boolean
  preferredVersions?: PreferredVersions
  wantedDependencies: Array<T & WantedDependency & { updateDepth: number }>
}

export interface ResolveDependenciesOptions {
  autoInstallPeers?: boolean
  allowBuild?: (pkgName: string) => boolean
  allowedDeprecatedVersions: AllowedDeprecatedVersions
  currentLockfile: Lockfile
  dryRun: boolean
  engineStrict: boolean
  force: boolean
  forceFullResolution: boolean
  hooks: {
    readPackage?: ReadPackageHook
  }
  nodeVersion: string
  registries: Registries
  patchedDependencies?: Record<string, PatchFile>
  pnpmVersion: string
  preferredVersions?: PreferredVersions
  preferWorkspacePackages?: boolean
  updateMatching?: (pkgName: string) => boolean
  linkWorkspacePackagesDepth?: number
  lockfileDir: string
  storeController: StoreController
  tag: string
  virtualStoreDir: string
  wantedLockfile: Lockfile
  workspacePackages: WorkspacePackages
}

export default async function<T> (
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
    dependenciesTree: {} as DependenciesTree<ResolvedPackage>,
    dryRun: opts.dryRun,
    engineStrict: opts.engineStrict,
    force: opts.force,
    forceFullResolution: opts.forceFullResolution,
    linkWorkspacePackagesDepth: opts.linkWorkspacePackagesDepth ?? -1,
    lockfileDir: opts.lockfileDir,
    nodeVersion: opts.nodeVersion,
    outdatedDependencies: {} as {[pkgId: string]: string},
    patchedDependencies: opts.patchedDependencies,
    pendingNodes: [] as PendingNode[],
    pnpmVersion: opts.pnpmVersion,
    preferWorkspacePackages: opts.preferWorkspacePackages,
    readPackageHook: opts.hooks.readPackage,
    registries: opts.registries,
    resolvedPackagesByDepPath: {} as ResolvedPackagesByDepPath,
    skipped: wantedToBeSkippedPackageIds,
    storeController: opts.storeController,
    updateMatching: opts.updateMatching,
    virtualStoreDir: opts.virtualStoreDir,
    wantedLockfile: opts.wantedLockfile,
    appliedPatches: new Set<string>(),
  }

  const resolveArgs: ImporterToResolve[] = importers.map((importer) => {
    const projectSnapshot = opts.wantedLockfile.importers[importer.id]
    // This array will only contain the dependencies that should be linked in.
    // The already linked-in dependencies will not be added.
    const linkedDependencies = [] as LinkedDependency[]
    const resolveCtx = {
      ...ctx,
      updatedSet: new Set<string>(),
      linkedDependencies,
      modulesDir: importer.modulesDir,
      prefix: importer.rootDir,
    }
    // This may be optimized.
    // We only need to proceed resolving every dependency
    // if the newly added dependency has peer dependencies.
    const proceed = importer.id === '.' || importer.hasRemovedDependencies === true || importer.wantedDependencies.some((wantedDep) => wantedDep['isNew'])
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
      workspacePackages: opts.workspacePackages,
    }
    return {
      ctx: resolveCtx,
      parentPkgAliases: fromPairs(
        importer.wantedDependencies.filter(({ alias }) => alias).map(({ alias }) => [alias, true])
      ) as ParentPkgAliases,
      preferredVersions: importer.preferredVersions ?? {},
      wantedDependencies: importer.wantedDependencies,
      options: resolveOpts,
    }
  })
  const pkgAddressesByImporters = await resolveRootDependencies(resolveArgs)
  const directDepsByImporterId = zipObj(importers.map(({ id }) => id), pkgAddressesByImporters)

  ctx.pendingNodes.forEach((pendingNode) => {
    ctx.dependenciesTree[pendingNode.nodeId] = {
      children: () => buildTree(ctx, pendingNode.nodeId, pendingNode.resolvedPackage.id,
        ctx.childrenByParentDepPath[pendingNode.resolvedPackage.depPath], pendingNode.depth + 1, pendingNode.installable),
      depth: pendingNode.depth,
      installable: pendingNode.installable,
      resolvedPackage: pendingNode.resolvedPackage,
    }
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
          const resolvedPackage = ctx.dependenciesTree[dep.nodeId].resolvedPackage as ResolvedPackage
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
        .reduce((acc, dependency) => {
          acc[dependency.alias] = dependency.nodeId
          return acc
        }, {}),
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
  children: Array<{alias: string, depPath: string}>,
  depth: number,
  installable: boolean
) {
  const childrenNodeIds = {}
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
    ctx.dependenciesTree[childNodeId] = {
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
    }
  }
  return childrenNodeIds
}
