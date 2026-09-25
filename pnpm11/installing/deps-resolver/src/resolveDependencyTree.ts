import { resolveFromCatalog } from '@pnpm/catalogs.resolver'
import type { Catalogs } from '@pnpm/catalogs.types'
import { pickRegistryContext } from '@pnpm/config.normalize-registries'
import { createPackageVersionPolicyOrThrow, getPublishedByPolicy } from '@pnpm/config.version-policy'
import * as dp from '@pnpm/deps.path'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { findLockedRootNodeRuntime } from '@pnpm/lockfile.utils'
import { globalWarn } from '@pnpm/logger'
import type { PatchGroupRecord } from '@pnpm/patching.config'
import { BUILTIN_REGISTRIES_BY_PREFIX } from '@pnpm/resolving.npm-resolver'
import type { PreferredVersions, Resolution, ResolutionPolicyViolation, WorkspacePackages } from '@pnpm/resolving.resolver-base'
import type { StoreController } from '@pnpm/store.controller-types'
import type { AllowBuild, AllowedDeprecatedVersions, DepPath, PkgResolutionId, ProjectId, ProjectManifest, ProjectRootDir, RangeSpecStyle, ReadPackageHook, RegistryContext, SupportedArchitectures, TrustPolicy } from '@pnpm/types'
import { partition } from 'ramda'

import { collectDirectDependencySpecs, findStalePeerPins, releaseStalePeerPins } from './findStalePeerPins.js'
import type { WantedDependency } from './getNonDevWantedDependencies.js'
import type { NodeId } from './nextNodeId.js'
import {
  buildTree,
  type ChildrenByParentId,
  type DependenciesTree,
  type ImporterToResolve,
  type ImporterToResolveOptions,
  type LinkedDependency,
  type ParentPkgAliases,
  type PendingNode,
  type PkgAddress,
  type PkgAddressOrLink,
  type ResolutionContext,
  type ResolvedPackage,
  type ResolvedPkgsById,
  resolveRootDependencies,
} from './resolveDependencies.js'

export type { DependenciesTree, DependenciesTreeNode, LinkedDependency, ResolvedPackage } from './resolveDependencies.js'

export interface ResolvedImporters {
  [id: string]: {
    directDependencies: ResolvedDirectDependency[]
    directNodeIdsByAlias: Map<string, NodeId>
    hoistedPeerProviderNodeIds: Set<NodeId>
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
  /**
   * The wanted dependency this was resolved from, carried so consumers can
   * recover the request directly. See `updateProjectManifest`.
   */
  wantedDependency?: WantedDependency
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
  updatePatches?: boolean
  updateToLatest?: boolean
  hasRemovedDependencies?: boolean
  preferredVersions?: PreferredVersions
  wantedDependencies: Array<WantedDepExtraProps & WantedDependency & { updateDepth: number }>
  rangeSpecStyle?: RangeSpecStyle
}

export interface ResolveDependenciesOptions extends RegistryContext {
  allowBuild?: AllowBuild
  autoInstallPeers?: boolean
  autoInstallPeersFromHighestMatch?: boolean
  allowedDeprecatedVersions: AllowedDeprecatedVersions
  allowUnusedPatches: boolean
  catalogs?: Catalogs
  currentLockfile: LockfileObject
  dedupePeerDependents?: boolean
  dryRun: boolean
  /**
   * Move a `node_modules` entry another package manager installed aside even
   * though this pass writes no `node_modules` itself. Set by a resolve pass
   * that a materialization pass follows into the same directory, which needs
   * the entry out of the way before it links (pnpm/pnpm#881).
   */
  hideAlienModules?: boolean
  engineStrict: boolean
  force: boolean
  forceFullResolution: boolean
  /**
   * Aliases whose lockfile pins are not reused, because an override that may
   * have produced them no longer applies.
   */
  staleOverrideTargets?: ReadonlySet<string>
  updateChecksums?: boolean
  ignoreScripts?: boolean
  hooks: {
    readPackage?: ReadPackageHook
  }
  overrideBareSpecifier?: (name: string, bareSpecifier: string, dir?: string) => string | undefined
  nodeVersion?: string
  /**
   * Check engines against the Node.js version the root project's `node`
   * runtime dependency resolves to, when it has one. Set when the user did not
   * configure `nodeVersion`.
   */
  checkEnginesAgainstRootRuntime?: boolean
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
  time?: Record<string, string>
  /**
   * Policy violations collected inline during resolution — the
   * resolver pushes to this list whenever it picks a package that
   * trips one of its own checks (today: `minimumReleaseAge`). The
   * shape mirrors `ResolutionPolicyViolation`; downstream callers
   * filter by `code` to decide what to do.
   */
  resolutionPolicyViolations: ResolutionPolicyViolation[]
}

export async function resolveDependencyTree<T> (
  importers: Array<ImporterToResolveGeneric<T>>,
  opts: ResolveDependenciesOptions
): Promise<ResolveDependencyTreeResult> {
  const wantedToBeSkippedPackageIds = new Set<PkgResolutionId>()
  const autoInstallPeers = opts.autoInstallPeers === true
  const { publishedBy, publishedByExclude } = getPublishedByPolicy(opts)
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
    staleOverrideTargets: opts.staleOverrideTargets,
    updateChecksums: opts.updateChecksums,
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
    overrideBareSpecifier: opts.overrideBareSpecifier,
    ...pickRegistryContext(opts),
    namedRegistryPrefixes: Array.from(
      new Set([
        ...Object.keys(BUILTIN_REGISTRIES_BY_PREFIX),
        ...Object.keys(opts.registriesByPrefix ?? {}),
      ])
    ).map((alias) => `${alias}:`),
    resolvedPkgsById: {} as ResolvedPkgsById,
    resolvePeersFromWorkspaceRoot: opts.resolvePeersFromWorkspaceRoot,
    resolutionMode: opts.resolutionMode,
    skipped: wantedToBeSkippedPackageIds,
    storeController: opts.storeController,
    virtualStoreDir: opts.virtualStoreDir,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    wantedLockfile: opts.wantedLockfile,
    updatedSet: new Set<string>(),
    lockedDepPathByPkgId: getLockedDepPathByPkgId(opts.wantedLockfile),
    workspacePackages: opts.workspacePackages,
    missingPeersOfChildrenByPkgId: {},
    hoistPeers: autoInstallPeers || opts.dedupePeerDependents,
    allPeerDepNames: new Set(),
    maximumPublishedBy: publishedBy,
    publishedByExclude,
    packageResolutionBarrier: {
      activeByDepth: new Map(),
      waiters: [],
    },
    childrenResolutionByPkgId: {},
    childrenResolutionId: 0,
    importerResolutionOrder: Object.fromEntries(importers.map(({ id }, index) => [id, index])),
    nodeResolutionContextByNodeId: new Map(),
    trustPolicy: opts.trustPolicy,
    trustPolicyExclude: opts.trustPolicyExclude ? createPackageVersionPolicyOrThrow(opts.trustPolicyExclude, 'trustPolicyExclude') : undefined,
    trustPolicyIgnoreAfter: opts.trustPolicyIgnoreAfter,
    blockExoticSubdeps: opts.blockExoticSubdeps,
    resolutionPolicyViolations: [],
  }

  if (opts.checkEnginesAgainstRootRuntime === true) {
    ctx.nodeVersion = await resolveRootRuntimeNodeVersion(importers, opts) ?? opts.nodeVersion
  }

  const directSpecsByName = autoInstallPeers && importers.some(({ manifest }) => manifest.peerDependencies != null)
    ? collectDirectDependencySpecs(importers.map(({ manifest }) => manifest), opts.catalogs ?? {})
    : undefined
  const resolveArgs: ImporterToResolve[] = importers.map((importer) => {
    const projectSnapshot = opts.wantedLockfile.importers[importer.id]
    const lockedDependencies = {
      ...projectSnapshot.dependencies,
      ...projectSnapshot.devDependencies,
      ...projectSnapshot.optionalDependencies,
    }
    const stalePeerPins = directSpecsByName == null
      ? undefined
      : findStalePeerPins(lockedDependencies, {
        directSpecsByName,
        lockfile: opts.wantedLockfile,
        manifest: importer.manifest,
      })
    const { preferredVersions, resolvedDependencies } = stalePeerPins?.size
      ? releaseStalePeerPins(stalePeerPins, { preferredVersions: importer.preferredVersions ?? {}, resolvedDependencies: lockedDependencies })
      : { preferredVersions: importer.preferredVersions ?? {}, resolvedDependencies: lockedDependencies }
    // This may be optimized.
    // We only need to proceed resolving every dependency
    // if the newly added dependency has peer dependencies.
    const proceed = importer.id === '.' || importer.hasRemovedDependencies === true || importer.wantedDependencies.some((wantedDep) => wantedDep.isNew)
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
      resolvedDependencies,
      updateDepth: -1,
      updateMatching: importer.updateMatching,
      updatePatches: importer.updatePatches,
      updateToLatest: importer.updateToLatest,
      prefix: importer.rootDir,
      supportedArchitectures: opts.supportedArchitectures,
    }
    return {
      updatePackageManifest: importer.updatePackageManifest,
      parentPkgAliases: Object.fromEntries(
        importer.wantedDependencies.filter(({ alias }) => alias).map(({ alias }) => [alias, true])
      ) as ParentPkgAliases,
      preferredVersions,
      wantedDependencies: importer.wantedDependencies,
      options: resolveOpts,
      rangeSpecStyle: importer.rangeSpecStyle,
    }
  })
  const { pkgAddressesByImporters, time } = await resolveRootDependencies(ctx, resolveArgs)
  const directDepsByImporterId = Object.fromEntries(importers.map(({ id }, i) => [id, pkgAddressesByImporters[i]]))

  for (const directDependencies of pkgAddressesByImporters) {
    for (const directDep of directDependencies as PkgAddress[]) {
      const { alias, normalizedBareSpecifier, version, saveCatalogName } = directDep

      // A dependency resolved through its `catalog:` reference already belongs to the catalog, and
      // an update moves that entry through `updatedCatalogs`.
      if (saveCatalogName == null || directDep.catalogLookup != null) {
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
      lockedPeerContext: pendingNode.lockedPeerContext,
      previousDepPath: pendingNode.previousDepPath,
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
            wantedDependency: dep.wantedDependency,
          }
        }),
      directNodeIdsByAlias: new Map(directNonLinkedDeps.map(({ alias, nodeId }) => [alias, nodeId])),
      hoistedPeerProviderNodeIds: new Set(directNonLinkedDeps.filter((dep) => dep.hoistedPeerProvider).map(({ nodeId }) => nodeId)),
      linkedDependencies,
    }
  }

  return {
    dependenciesTree: ctx.dependenciesTree,
    outdatedDependencies: ctx.outdatedDependencies,
    resolvedImporters,
    resolvedPkgsById: ctx.resolvedPkgsById,
    wantedToBeSkippedPackageIds,
    time,
    allPeerDepNames: ctx.allPeerDepNames,
    resolutionPolicyViolations: ctx.resolutionPolicyViolations,
  }
}

/**
  * There may be cases where multiple dependencies have the same alias in the directDeps array.
  * E.g., when there is "is-negative: github:kevva/is-negative#1.0.0" in the package.json dependencies,
  * and then re-execute `pnpm add github:kevva/is-negative#1.0.1`.
  * In order to make sure that the latest 1.0.1 version is installed, we need to remove the duplicate dependency.
  * fix https://github.com/pnpm/pnpm/issues/6966
  */
function dedupeSameAliasDirectDeps (directDeps: PkgAddressOrLink[], wantedDependencies: WantedDependency[]): PkgAddressOrLink[] {
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

function getLockedDepPathByPkgId (lockfile: LockfileObject): Map<PkgResolutionId, DepPath> {
  const lockedDepPathByPkgId = new Map<PkgResolutionId, DepPath>()
  for (const depPath of Object.keys(lockfile.packages ?? {}) as DepPath[]) {
    const pkgId = dp.tryGetPackageId(depPath) as string as PkgResolutionId
    if (!lockedDepPathByPkgId.has(pkgId)) {
      lockedDepPathByPkgId.set(pkgId, depPath)
    }
  }
  return lockedDepPathByPkgId
}

/**
 * The Node.js version the root project's `node` runtime dependency resolves
 * to in this install. It is resolved ahead of the other dependencies because
 * each package's engines are checked when the package is requested.
 */
async function resolveRootRuntimeNodeVersion<T> (
  importers: Array<ImporterToResolveGeneric<T>>,
  opts: Pick<ResolveDependenciesOptions, 'lockfileDir' | 'storeController' | 'wantedLockfile'>
): Promise<string | undefined> {
  const locked = findLockedRootNodeRuntime(opts.wantedLockfile)
  const rootImporter = importers.find(({ id }) => id === '.')
  if (rootImporter == null) return locked?.version
  const wantedNode = rootImporter.wantedDependencies.find(({ alias, bareSpecifier }) =>
    alias === 'node' && bareSpecifier.startsWith('runtime:'))
  if (wantedNode == null) return undefined
  const updateRequested = wantedNode.updateDepth >= 0 && (rootImporter.updateMatching?.('node', locked?.version) ?? true)
  if (locked != null && locked.specifier === wantedNode.bareSpecifier && !updateRequested) {
    return locked.version
  }
  const { body } = await opts.storeController.requestPackage(wantedNode, {
    downloadPriority: 0,
    lockfileDir: opts.lockfileDir,
    preferredVersions: {},
    projectDir: rootImporter.rootDir,
    skipFetch: true,
  })
  return body.manifest?.version
}
