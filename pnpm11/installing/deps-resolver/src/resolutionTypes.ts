import type { CatalogResolver } from '@pnpm/catalogs.resolver'
import type { LockfileObject, PackageSnapshot, ResolvedDependencies } from '@pnpm/lockfile.types'
import type { PatchGroupRecord } from '@pnpm/patching.config'
import type { PatchInfo } from '@pnpm/patching.types'
import type {
  DirectoryResolution,
  PkgResolutionId,
  PreferredVersions,
  Resolution,
  ResolutionPolicyViolation,
  WorkspacePackages,
} from '@pnpm/resolving.resolver-base'
import type { PkgRequestFetchResult, StoreController } from '@pnpm/store.controller-types'
import type { AllowBuild, AllowedDeprecatedVersions, DepPath, PackageManifest, PackageVersionPolicy, PkgIdWithPatchHash, RangeSpecStyle, ReadPackageHook, RegistryContext, SupportedArchitectures, TrustPolicy } from '@pnpm/types'

import type { WantedDependency } from './getWantedDependencies.js'
import type { NodeId } from './nextNodeId.js'
import type { CatalogLookupMetadata } from './resolveDependencyTree.js'

// child nodeId by child alias name in case of non-linked deps
export interface ChildrenMap {
  [alias: string]: NodeId
}

export type DependenciesTreeNode<Pkg> = {
  children: (() => ChildrenMap) | ChildrenMap
  installable: boolean
  dependencyNamesWhoseCurrentProviderMustWin?: Set<string>
  lockedPeerContext?: LockedPeerContext
  previousDepPath?: DepPath
} & ({
  resolvedPackage: Pkg & { name: string, version: string }
  depth: number
} | {
  resolvedPackage: { name: string, version: string }
  depth: -1
})

export type DependenciesTree<Pkg> = Map<
// a node ID is the join of the package's keypath with a colon
// E.g., a subdeps node ID which parent is `foo` will be
// registry.npmjs.org/foo/1.0.0:registry.npmjs.org/bar/1.0.0
  NodeId,
  DependenciesTreeNode<Pkg>
>

export type ResolvedPkgsById = Record<PkgResolutionId, ResolvedPackage>

export interface PkgAddressOrLinkBase {
  alias: string
  catalogLookup?: CatalogLookupMetadata
  normalizedBareSpecifier?: string
  optional: boolean
  pkg: PackageManifest
  pkgId: PkgResolutionId
  /**
   * The wanted dependency this direct dependency was resolved from, carried so
   * consumers can recover the request directly. See `updateProjectManifest`.
   */
  wantedDependency?: WantedDependency
}

export interface LinkedDependency extends PkgAddressOrLinkBase {
  isLinkedDependency: true
  dev: boolean
  resolution: DirectoryResolution
  version: string
  name: string
}

export interface PendingNode {
  alias: string
  nodeId: NodeId
  resolvedPackage: ResolvedPackage
  depth: number
  installable: boolean
  lockedPeerContext?: LockedPeerContext
  previousDepPath?: DepPath
  parentIds: PkgResolutionId[]
}

export interface ChildrenByParentId {
  [id: PkgResolutionId]: Array<{
    alias: string
    id: PkgResolutionId
  }>
}

export interface ResolutionContext extends RegistryContext {
  allowBuild?: AllowBuild
  allPeerDepNames: Set<string>
  autoInstallPeers: boolean
  autoInstallPeersFromHighestMatch: boolean
  allowedDeprecatedVersions: AllowedDeprecatedVersions
  allPreferredVersions?: PreferredVersions
  updatedSet: Set<string>
  /**
   * One snapshot of each package in the wanted lockfile. The dependencies of
   * a package other than its peers are the same in every snapshot of it.
   */
  lockedDepPathByPkgId: Map<PkgResolutionId, DepPath>
  catalogResolver: CatalogResolver
  defaultTag: string
  dryRun: boolean
  forceFullResolution: boolean
  lockedPeersAreCurrent: boolean
  staleOverrideTargets?: ReadonlySet<string>
  updateChecksums?: boolean
  ignoreScripts?: boolean
  resolvedPkgsById: ResolvedPkgsById
  resolvePeersFromWorkspaceRoot?: boolean
  outdatedDependencies: Record<PkgResolutionId, string>
  packageResolutionBarrier: PackageResolutionBarrier
  childrenByParentId: ChildrenByParentId
  childrenResolutionByPkgId: Record<PkgResolutionId, ChildrenResolution>
  childrenResolutionId: number
  importerResolutionOrder: Record<string, number>
  nodeResolutionContextByNodeId: Map<NodeId, NodeResolutionContext>
  patchedDependencies?: PatchGroupRecord
  pendingNodes: PendingNode[]
  wantedLockfile: LockfileObject
  currentLockfile: LockfileObject
  injectWorkspacePackages?: boolean
  linkWorkspacePackagesDepth: number
  lockfileDir: string
  storeController: StoreController
  // the IDs of packages that are not installable
  skipped: Set<PkgResolutionId>
  dependenciesTree: DependenciesTree<ResolvedPackage>
  force: boolean
  preferWorkspacePackages?: boolean
  readPackageHook?: ReadPackageHook
  overrideBareSpecifier?: (name: string, bareSpecifier: string, dir?: string) => string | undefined
  engineStrict: boolean
  nodeVersion?: string
  pnpmVersion: string
  namedRegistryPrefixes: readonly string[]
  resolutionMode?: 'highest' | 'time-based' | 'lowest-direct'
  virtualStoreDir: string
  virtualStoreDirMaxLength: number
  workspacePackages?: WorkspacePackages
  missingPeersOfChildrenByPkgId: Record<PkgResolutionId, { depth: number, missingPeersOfChildren: MissingPeersOfChildren }>
  hoistPeers?: boolean
  maximumPublishedBy?: Date
  publishedByExclude?: PackageVersionPolicy
  /**
   * Shared accumulator the resolver pushes into when an inline policy
   * check flags a pick. resolveDependencyTree hands the populated array
   * back to the install command via its return so the post-tree gate can
   * prompt / abort / persist without re-walking the resolved tree.
   */
  resolutionPolicyViolations: ResolutionPolicyViolation[]
  trustPolicy?: TrustPolicy
  trustPolicyExclude?: PackageVersionPolicy
  trustPolicyIgnoreAfter?: number
  blockExoticSubdeps?: boolean
}

export interface PackageResolutionBarrier {
  activeByDepth: Map<number, number>
  waiters: Array<() => void>
}

export interface ChildrenResolutionOwner {
  depth: number
  importerOrder: number
  parentPath: PkgResolutionId[]
}

export interface ChildrenResolution {
  id: number
  owner: ChildrenResolutionOwner
  missingPeersOfChildren?: MissingPeersOfChildren
}

export interface NodeResolutionContext {
  depth: number
  installable: boolean
  parentIds: PkgResolutionId[]
  pkgId: PkgResolutionId
}

export interface MissingPeerInfo {
  range: string
  optional: boolean
}

export type MissingPeers = Record<string, MissingPeerInfo>

export type ResolvedPeers = Record<string, PkgAddress>

export interface MissingPeersOfChildren {
  resolve: (missingPeers: MissingPeers) => void
  reject: (err: Error) => void
  get: () => Promise<MissingPeers>
  resolved?: boolean
}

export interface PkgAddress extends PkgAddressOrLinkBase {
  depIsLinked: boolean
  isNew: boolean
  isLinkedDependency?: false
  resolvedVia?: string
  nodeId: NodeId
  installable: boolean
  version?: string
  updated: boolean
  rootDir: string
  missingPeers: MissingPeers
  missingPeersOfChildren?: MissingPeersOfChildren
  childrenResolutionId?: number
  publishedAt?: string
  saveCatalogName?: string
  lockedPeerContext?: LockedPeerContext
  previousDepPath?: DepPath
  /**
   * A peer dependency provider attached to the root importer so that other
   * subtrees can reuse it. Its `nodeId` keeps pointing at the provider's
   * original position inside the dependency tree, so the node must be
   * peer-resolved there — not in the root context.
   */
  hoistedPeerProvider?: boolean
}

export type PkgAddressOrLink = PkgAddress | LinkedDependency

export interface PeerDependency {
  version: string
  optional?: boolean
}

export type PeerDependencies = Record<string, PeerDependency>

export interface ResolvedPackage {
  id: PkgResolutionId
  isLeaf: boolean
  resolution: Resolution
  /** Which resolver produced this package; see `detectNamedRegistryCollision`. */
  resolvedVia?: string
  prod: boolean
  dev: boolean
  optional: boolean
  fetching: () => Promise<PkgRequestFetchResult>
  /**
   * The resolution can't be completed without awaiting `fetching` (e.g. a registry tarball
   * whose integrity is computed from the downloaded bytes). The lockfile snapshot and the
   * virtual-store paths derived from the integrity must await `fetching` for these first.
   */
  resolutionNeedsFetch?: boolean
  filesIndexFile: string
  name: string
  version: string
  peerDependencies: PeerDependencies
  optionalDependencies: Set<string>
  hasBin: boolean
  hasBundledDependencies: boolean
  patch?: PatchInfo
  prepare?: boolean
  pkgIdWithPatchHash: PkgIdWithPatchHash
  requiresBuild?: boolean
  additionalInfo: {
    deprecated?: string
    bundleDependencies?: string[] | boolean
    bundledDependencies?: string[] | boolean
    engines?: {
      node?: string
      npm?: string
    }
    cpu?: string[]
    os?: string[]
    libc?: string[]
  }
}

export type ParentPkg = Pick<PkgAddress, 'nodeId' | 'installable' | 'rootDir' | 'optional' | 'pkgId' | 'resolvedVia' | 'lockedPeerContext' | 'previousDepPath'>

export type ParentPkgAliases = Record<string, PkgAddress | true>

export type UpdateMatchingFunction = (pkgName: string, version?: string) => boolean

export interface ResolvedDependenciesOptions {
  currentDepth: number
  directDepVersions?: Record<string, string[]>
  parentIds: PkgResolutionId[]
  parentPkg: ParentPkg
  parentPkgAliases: ParentPkgAliases
  // If the package has been updated, the dependencies
  // which were used by the previous version are passed
  // via this option
  preferredDependencies?: ResolvedDependencies
  /**
   * The dependencies that the lockfile records for the parent package, when
   * the edge to the parent has no lockfile entry of its own. They are reused
   * as preferred versions only, so the peers are resolved from scratch.
   */
  lockedDependencies?: ResolvedDependencies
  proceed: boolean
  publishedBy?: Date
  pickLowestVersion?: boolean
  resolvedDependencies?: ResolvedDependencies
  updateMatching?: UpdateMatchingFunction
  updatePatches?: boolean
  updateDepth: number
  prefix: string
  supportedArchitectures?: SupportedArchitectures
  updateToLatest?: boolean
  rangeSpecStyle?: RangeSpecStyle
}

export interface PostponedResolutionOpts {
  directDepVersions?: Record<string, string[]>
  preferredVersions: PreferredVersions
  parentPkgAliases: ParentPkgAliases
  publishedBy?: Date
}

export interface PeersResolutionResult {
  missingPeers: MissingPeers
  resolvedPeers: ResolvedPeers
}

export type PostponedResolutionFunction = (opts: PostponedResolutionOpts) => Promise<PeersResolutionResult>
export type PostponedPeersResolutionFunction = (parentPkgAliases: ParentPkgAliases) => Promise<PeersResolutionResult>

export interface ResolvedRootDependenciesResult {
  pkgAddressesByImporters: PkgAddressOrLink[][]
  time?: Record<string, string>
}

export interface ResolvedDependenciesResult {
  pkgAddresses: PkgAddressOrLink[]
  resolvingPeers: Promise<PeersResolutionResult>
}

export interface PkgAddressesByImportersWithoutPeers extends PeersResolutionResult {
  pkgAddresses: PkgAddressOrLink[]
}

export type ImporterToResolveOptions = Omit<ResolvedDependenciesOptions, 'parentPkgAliases' | 'publishedBy'>

export interface ImporterToResolve {
  updatePackageManifest: boolean
  preferredVersions: PreferredVersions
  parentPkgAliases: ParentPkgAliases
  wantedDependencies: Array<WantedDependency & { updateDepth?: number }>
  options: ImporterToResolveOptions
  rangeSpecStyle?: RangeSpecStyle
}

export interface ResolveDependenciesOfImportersResult {
  pkgAddressesByImportersWithoutPeers: PkgAddressesByImportersWithoutPeers[]
  publishedBy?: Date
  time?: Record<string, string>
}

export interface ExtendedWantedDependency {
  infoFromLockfile?: InfoFromLockfile
  /**
   * A version from the lockfile to reuse. This is ignored if an update of the
   * wanted dependency is requested.
   */
  preferredVersion?: string
  proceed: boolean
  wantedDependency: WantedDependency & { updateDepth?: number }
}

export interface ResolveDependenciesOfDependency {
  postponedResolution?: PostponedResolutionFunction
  postponedPeersResolution?: PostponedPeersResolutionFunction
  resolveDependencyResult: ResolveDependencyResult
}

export interface LockedPeerContext {
  [peerName: string]: DepPath
}

export type InfoFromLockfile = {
  depPath: DepPath
  pkgId: PkgResolutionId
  dependencyLockfile?: PackageSnapshot
  lockedPeerContext?: LockedPeerContext
  name?: string
  version?: string
  resolution?: Resolution
} & ({
  dependencyLockfile: PackageSnapshot
  name: string
  version: string
  resolution: Resolution
} | unknown)

export interface ResolveDependencyOptions {
  currentDepth: number
  currentPkg?: {
    depPath?: DepPath
    name?: string
    version?: string
    pkgId?: PkgResolutionId
    resolution?: Resolution
    dependencyLockfile?: PackageSnapshot
    lockedPeerContext?: LockedPeerContext
  }
  preferredVersion?: string
  parentPkg: ParentPkg
  parentIds: PkgResolutionId[]
  parentPkgAliases: ParentPkgAliases
  preferredVersions: PreferredVersions
  prefix: string
  proceed: boolean
  publishedBy?: Date
  pickLowestVersion?: boolean
  update: false | 'compatible' | 'latest'
  updatePatches?: boolean
  updateChecksums?: boolean
  updateDepth: number
  /**
   * Whether or not an update is requested based on filter conditions (such as
   * update depth and package name) on an existing dependency with a resolution
   * present in the lockfile.
   *
   * This is different than the "update" option, which may be set for new
   * dependencies or packages that need to be re-fetched.
   */
  updateRequested: boolean
  supportedArchitectures?: SupportedArchitectures
  rangeSpecStyle?: RangeSpecStyle
}

export type ResolveDependencyResult = PkgAddressOrLink | null
