import { PnpmError } from '@pnpm/error'
import { resolveFromCatalog } from '@pnpm/catalogs.resolver'
import { type Catalogs } from '@pnpm/catalogs.types'
import { type LockfileObject } from '@pnpm/lockfile.types'
import { globalWarn } from '@pnpm/logger'
import { createPackageVersionPolicy } from '@pnpm/config.version-policy'
import { type PatchGroupRecord } from '@pnpm/patching.config'
import { type PreferredVersions, type Resolution, type WorkspacePackages } from '@pnpm/resolver-base'
import { type StoreController } from '@pnpm/store-controller-types'
import {
  type AllowBuild,
  type SupportedArchitectures,
  type AllowedDeprecatedVersions,
  type PinnedVersion,
  type PkgResolutionId,
  type ProjectManifest,
  type ProjectId,
  type ReadPackageHook,
  type Registries,
  type ProjectRootDir,
  type PackageVersionPolicy,
  type TrustPolicy,
} from '@pnpm/types'
import { partition, zipObj } from 'ramda'
import { type WantedDependency } from './getNonDevWantedDependencies.js'
import { type NodeId, nextNodeId } from './nextNodeId.js'
import { parentIdsContainSequence } from './parentIdsContainSequence.js'
import {
  type ChildrenByParentId,
  type DependenciesTree,
  type LinkedDependency,
  type ImporterToResolve,
  type ImporterToResolveOptions,
  type ParentPkgAliases,
  type PendingNode,
  type PkgAddress,
  type PkgAddressOrLink,
  resolveRootDependencies,
  type ResolvedPackage,
  type ResolvedPkgsById,
  type ResolutionContext,
} from './resolveDependencies.js'

export type { LinkedDependency, ResolvedPackage, DependenciesTree, DependenciesTreeNode } from './resolveDependencies.js'

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
  catalogLookup?: CatalogLookupMetadata
  normalizedBareSpecifier?: string
}

/**
 * Information related to the catalog entry for this dependency if it was
 * requested through the catalog protocol.
 */
export interface CatalogLookupMetadata {
  readonly catalogName: string
  readonly specifier: string

  /**
   * The catalog protocol bareSpecifier the user wrote in package.json files or as a
   * parameter to pnpm add. Ex: pnpm add foo@catalog:
   *
   * This will usually be 'catalog:<name>', but can simply be 'catalog:' if
   * users wrote the default catalog shorthand. This is different than the
   * catalogName field, which would be 'default' regardless of whether users
   * originally requested 'catalog:' or 'catalog:default'.
   */
  readonly userSpecifiedBareSpecifier: string
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
  updateMatching?: (pkgName: string, version?: string) => boolean
  updateToLatest?: boolean
  hasRemovedDependencies?: boolean
  preferredVersions?: PreferredVersions
  wantedDependencies: Array<WantedDepExtraProps & WantedDependency & { updateDepth: number }>
  pinnedVersion?: PinnedVersion
}

export interface ResolveDependenciesOptions {
  allowBuild?: AllowBuild
  autoInstallPeers?: boolean
  autoInstallPeersFromHighestMatch?: boolean
  allowedDeprecatedVersions: AllowedDeprecatedVersions
  allowUnusedPatches: boolean
  catalogs?: Catalogs
  currentLockfile: LockfileObject
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
  patchedDependencies?: PatchGroupRecord
  pnpmVersion: string
  preferredVersions?: PreferredVersions
  preferWorkspacePackages?: boolean
  resolutionMode?: 'highest' | 'time-based' | 'lowest-direct'
  resolvePeersFromWorkspaceRoot?: boolean
  injectWorkspacePackages?: boolean
  linkWorkspacePackagesDepth?: number
  lockfileDir: string
  storeController: StoreController
  tag: string
  virtualStoreDir: string
  globalVirtualStoreDir: string
  virtualStoreDirMaxLength: number
  wantedLockfile: LockfileObject
  workspacePackages: WorkspacePackages
  supportedArchitectures?: SupportedArchitectures
  peersSuffixMaxLength: number
  minimumReleaseAge?: number
  minimumReleaseAgeExclude?: string[]
  trustPolicy?: TrustPolicy
  trustPolicyExclude?: string[]
  trustPolicyIgnoreAfter?: number
  blockExoticSubdeps?: boolean
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
    allowBuild: opts.allowBuild,
    autoInstallPeers,
    autoInstallPeersFromHighestMatch: opts.autoInstallPeersFromHighestMatch === true,
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
    injectWorkspacePackages: opts.injectWorkspacePackages,
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
    resolvePeersFromWorkspaceRoot: opts.resolvePeersFromWorkspaceRoot,
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
    maximumPublishedBy: opts.minimumReleaseAge ? new Date(Date.now() - opts.minimumReleaseAge * 60 * 1000) : undefined,
    publishedByExclude: opts.minimumReleaseAgeExclude ? createPackageVersionPolicyByExclude(opts.minimumReleaseAgeExclude, 'minimumReleaseAgeExclude') : undefined,
    trustPolicy: opts.trustPolicy,
    trustPolicyExclude: opts.trustPolicyExclude ? createPackageVersionPolicyByExclude(opts.trustPolicyExclude, 'trustPolicyExclude') : undefined,
    trustPolicyIgnoreAfter: opts.trustPolicyIgnoreAfter,
    blockExoticSubdeps: opts.blockExoticSubdeps,
  }

  function createPackageVersionPolicyByExclude (patterns: string[], key: string): PackageVersionPolicy {
    try {
      return createPackageVersionPolicy(patterns)
    } catch (err) {
      if (!err || typeof err !== 'object' || !('message' in err)) throw err
      throw new PnpmError(`INVALID_${key.replace(/([A-Z])/g, '_$1').toUpperCase()}`, `Invalid value in ${key}: ${err.message as string}`)
    }
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
      updateToLatest: importer.updateToLatest,
      prefix: importer.rootDir,
      supportedArchitectures: opts.supportedArchitectures,
    }
    return {
      updatePackageManifest: importer.updatePackageManifest,
      parentPkgAliases: Object.fromEntries(
        importer.wantedDependencies.filter(({ alias }) => alias).map(({ alias }) => [alias, true])
      ) as ParentPkgAliases,
      preferredVersions: importer.preferredVersions ?? {},
      wantedDependencies: importer.wantedDependencies,
      options: resolveOpts,
      pinnedVersion: importer.pinnedVersion,
    }
  })
  const { pkgAddressesByImporters, time } = await resolveRootDependencies(ctx, resolveArgs)
  const directDepsByImporterId = zipObj(importers.map(({ id }) => id), pkgAddressesByImporters)

  for (const directDependencies of pkgAddressesByImporters) {
    for (const directDep of directDependencies as PkgAddress[]) {
      const { alias, normalizedBareSpecifier, version, saveCatalogName } = directDep

      if (saveCatalogName == null) {
        continue
      }

      const existingCatalog = opts.catalogs?.default?.[alias]
      if (existingCatalog != null) {
        if (existingCatalog !== normalizedBareSpecifier) {
          globalWarn(
            `Skip adding ${alias} to the default catalog because it already exists as ${existingCatalog}. Please use \`pnpm update\` to update the catalogs.`
          )
        }
      } else if (normalizedBareSpecifier != null && version != null) {
        const userSpecifiedBareSpecifier = `catalog:${saveCatalogName === 'default' ? '' : saveCatalogName}`

        // Attach metadata about how this new catalog dependency should be
        // resolved so the pnpm-lock.yaml file's catalogs section can be updated
        // to reflect this newly added entry.
        directDep.catalogLookup = {
          catalogName: saveCatalogName,
          specifier: normalizedBareSpecifier,
          userSpecifiedBareSpecifier,
        }
      }
    }
  }

  for (const pendingNode of ctx.pendingNodes) {
    ctx.dependenciesTree.set(pendingNode.nodeId, {
      children: () => buildTree(ctx, pendingNode.resolvedPackage.id,
        pendingNode.parentIds,
        ctx.childrenByParentId[pendingNode.resolvedPackage.id], pendingNode.depth + 1, pendingNode.installable),
      depth: pendingNode.depth,
      installable: pendingNode.installable,
      resolvedPackage: pendingNode.resolvedPackage,
    })
  }

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
            optional: resolvedPackage.optional,
            pkgId: resolvedPackage.id,
            resolution: resolvedPackage.resolution,
            version: resolvedPackage.version,
            normalizedBareSpecifier: dep.normalizedBareSpecifier,
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
    if (ctx.resolvedPkgsById[child.id].isLeaf) {
      childrenNodeIds[child.alias] = child.id as unknown as NodeId
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
function dedupeSameAliasDirectDeps (directDeps: PkgAddressOrLink[], wantedDependencies: Array<WantedDependency & { isNew?: boolean }>): PkgAddressOrLink[] {
  const deps = new Map<string, PkgAddressOrLink>()
  for (const directDep of directDeps) {
    const { alias, normalizedBareSpecifier } = directDep
    if (!deps.has(alias)) {
      deps.set(alias, directDep)
    } else {
      const wantedDep = wantedDependencies.find(dep =>
        dep.alias ? dep.alias === alias : dep.bareSpecifier === normalizedBareSpecifier
      )
      if (wantedDep?.isNew) {
        deps.set(alias, directDep)
      }
    }
  }
  return Array.from(deps.values())
}
