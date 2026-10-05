import { resolveFromCatalog } from '@pnpm/catalogs.resolver'
import { pickRegistryContext } from '@pnpm/config.normalize-registries'
import { createPackageVersionPolicyOrThrow, getPublishedByPolicy } from '@pnpm/config.version-policy'
import * as dp from '@pnpm/deps.path'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { BUILTIN_REGISTRIES_BY_PREFIX } from '@pnpm/resolving.npm-resolver'
import type { DepPath, PkgResolutionId, ProjectId } from '@pnpm/types'

import type {
  ChildrenByParentId,
  DependenciesTree,
  PendingNode,
  ResolutionContext,
  ResolvedPackage,
  ResolvedPkgsById,
} from './resolutionTypes.js'
import type { ResolveDependenciesOptions } from './resolveDependencyTree.js'

type ResolutionPolicies = Pick<ResolutionContext,
| 'allowBuild'
| 'allowedDeprecatedVersions'
| 'blockExoticSubdeps'
| 'engineStrict'
| 'maximumPublishedBy'
| 'publishedByExclude'
| 'trustPolicy'
| 'trustPolicyExclude'
| 'trustPolicyIgnoreAfter'
>

type ResolutionState = Pick<ResolutionContext,
| 'allPeerDepNames'
| 'childrenByParentId'
| 'childrenResolutionByPkgId'
| 'childrenResolutionId'
| 'dependenciesTree'
| 'importerResolutionOrder'
| 'lockedDepPathByPkgId'
| 'missingPeersOfChildrenByPkgId'
| 'nodeResolutionContextByNodeId'
| 'outdatedDependencies'
| 'packageResolutionBarrier'
| 'pendingNodes'
| 'resolutionPolicyViolations'
| 'resolvedPkgsById'
| 'skipped'
| 'updatedSet'
>

type ResolutionSettings = Omit<ResolutionContext, keyof ResolutionPolicies | keyof ResolutionState>

export function createResolutionContext (
  importers: Array<{ id: ProjectId }>,
  opts: ResolveDependenciesOptions,
  skipped: Set<PkgResolutionId>
): ResolutionContext {
  const settings = getResolutionSettings(opts)
  const policies = getResolutionPolicies(opts)
  return {
    ...settings,
    ...policies,
    ...createResolutionState({ importers, skipped, wantedLockfile: opts.wantedLockfile }),
  }
}

function getResolutionSettings (opts: ResolveDependenciesOptions): ResolutionSettings {
  const autoInstallPeers = opts.autoInstallPeers === true
  return {
    autoInstallPeers,
    autoInstallPeersFromHighestMatch: opts.autoInstallPeersFromHighestMatch === true,
    catalogResolver: resolveFromCatalog.bind(null, opts.catalogs ?? {}),
    currentLockfile: opts.currentLockfile,
    defaultTag: opts.tag,
    dryRun: opts.dryRun,
    force: opts.force,
    forceFullResolution: opts.forceFullResolution,
    lockedPeersAreCurrent: opts.lockedPeersAreCurrent === true,
    staleOverrideTargets: opts.staleOverrideTargets,
    updateChecksums: opts.updateChecksums,
    ignoreScripts: opts.ignoreScripts,
    injectWorkspacePackages: opts.injectWorkspacePackages,
    linkWorkspacePackagesDepth: opts.linkWorkspacePackagesDepth ?? -1,
    lockfileDir: opts.lockfileDir,
    nodeVersion: opts.nodeVersion,
    patchedDependencies: opts.patchedDependencies,
    pnpmVersion: opts.pnpmVersion,
    preferWorkspacePackages: opts.preferWorkspacePackages,
    readPackageHook: opts.hooks.readPackage,
    overrideBareSpecifier: opts.overrideBareSpecifier,
    ...pickRegistryContext(opts),
    namedRegistryPrefixes: getNamedRegistryPrefixes(opts.registriesByPrefix),
    resolvePeersFromWorkspaceRoot: opts.resolvePeersFromWorkspaceRoot,
    resolutionMode: opts.resolutionMode,
    storeController: opts.storeController,
    virtualStoreDir: opts.virtualStoreDir,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    wantedLockfile: opts.wantedLockfile,
    workspacePackages: opts.workspacePackages,
    hoistPeers: autoInstallPeers || opts.dedupePeerDependents,
  }
}

function getNamedRegistryPrefixes (registriesByPrefix: ResolveDependenciesOptions['registriesByPrefix']): string[] {
  return Array.from(
    new Set([
      ...Object.keys(BUILTIN_REGISTRIES_BY_PREFIX),
      ...Object.keys(registriesByPrefix ?? {}),
    ])
  ).map((alias) => `${alias}:`)
}

function getResolutionPolicies (opts: ResolveDependenciesOptions): ResolutionPolicies {
  const { publishedBy, publishedByExclude } = getPublishedByPolicy(opts)
  return {
    allowBuild: opts.allowBuild,
    allowedDeprecatedVersions: opts.allowedDeprecatedVersions,
    engineStrict: opts.engineStrict,
    maximumPublishedBy: publishedBy,
    publishedByExclude,
    trustPolicy: opts.trustPolicy,
    trustPolicyExclude: opts.trustPolicyExclude ? createPackageVersionPolicyOrThrow(opts.trustPolicyExclude, 'trustPolicyExclude') : undefined,
    trustPolicyIgnoreAfter: opts.trustPolicyIgnoreAfter,
    blockExoticSubdeps: opts.blockExoticSubdeps,
  }
}

function createResolutionState (
  { importers, skipped, wantedLockfile }: {
    importers: Array<{ id: ProjectId }>
    skipped: Set<PkgResolutionId>
    wantedLockfile: LockfileObject
  }
): ResolutionState {
  return {
    allPeerDepNames: new Set(),
    childrenByParentId: {} as ChildrenByParentId,
    childrenResolutionByPkgId: {},
    childrenResolutionId: 0,
    dependenciesTree: new Map() as DependenciesTree<ResolvedPackage>,
    importerResolutionOrder: Object.fromEntries(importers.map(({ id }, index) => [id, index])),
    lockedDepPathByPkgId: getLockedDepPathByPkgId(wantedLockfile),
    missingPeersOfChildrenByPkgId: {},
    nodeResolutionContextByNodeId: new Map(),
    outdatedDependencies: {} as { [pkgId: string]: string },
    packageResolutionBarrier: {
      activeByDepth: new Map(),
      waiters: [],
    },
    pendingNodes: [] as PendingNode[],
    resolutionPolicyViolations: [],
    resolvedPkgsById: {} as ResolvedPkgsById,
    skipped,
    updatedSet: new Set<string>(),
  }
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
