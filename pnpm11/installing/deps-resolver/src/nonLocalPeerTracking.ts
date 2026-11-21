import * as dp from '@pnpm/deps.path'
import { PnpmError } from '@pnpm/error'
import type { PackageSnapshot } from '@pnpm/lockfile.types'
import type { PkgResolutionId } from '@pnpm/resolving.resolver-base'
import type { PackageResponse } from '@pnpm/store.controller-types'
import type { DependencyManifest, DepPath, PackageManifest } from '@pnpm/types'

import type { HoistableRootDep } from './hoistPeers.js'
import { createLazyReachability, type DirectDepRef, type LazyReachability } from './nonLocalPeerClosure.js'
import {
  describeNonLocalPackage,
  isForbiddenManifestSpec,
  overrideAuthorizesSpec,
  type ParentIdentity,
} from './nonLocalSubdeps.js'
import type {
  ImporterToResolve,
  MissingPeerInfo,
  ResolutionContext,
} from './resolutionTypes.js'

export interface HoistedPeerCheckInput {
  ctx: ResolutionContext
  importer: ImporterToResolve
  missingRequiredPeers: Array<[string, MissingPeerInfo]>
  pkgAddresses: DirectDepRef[]
  workspaceRootDeps: HoistableRootDep[]
}

/**
 * Reject hoisted required peers whose specifiers resolve against the host
 * filesystem unless authorized for this importer by `overrides`,
 * workspace-root dependencies, a local package reachable in this importer, or
 * per-parent hook/override rewrites on every declaring non-local package.
 */
export function assertHoistedPeersAllowed (
  input: HoistedPeerCheckInput,
  hoistedDependencies: Record<string, string>
): void {
  const reachability = createLazyReachability(input.ctx, input.importer, input.pkgAddresses)
  for (const [alias, hoistedSpec] of Object.entries(hoistedDependencies)) {
    const parent = findDisallowedHoistedPeerParent(input, reachability, alias, hoistedSpec)
    if (parent == null) continue
    throw new PnpmError(
      'LOCAL_SUBDEP_NOT_ALLOWED',
      `Cannot resolve local filesystem dependency "${hoistedSpec}" for "${alias}" declared by non-local package "${parent}"`
    )
  }
}

export function isHoistedPeerAllowed (
  input: HoistedPeerCheckInput,
  alias: string,
  hoistedSpec: string
): boolean {
  const reachability = createLazyReachability(input.ctx, input.importer, input.pkgAddresses)
  return findDisallowedHoistedPeerParent(input, reachability, alias, hoistedSpec) == null
}

/**
 * Create a batch-cached validator for detached optional-peer candidate probes.
 * Unlike `assertHoistedPeersAllowed`, a candidate outside this importer's
 * current reachable closure is not yet in `reachablePkgIds`, so a forbidden
 * local specifier requires positive authorization from workspace configuration
 * or from every declaring wave parent of that peer.
 */
export function createDetachedPeerSpecChecker (
  ctx: ResolutionContext,
  importer: ImporterToResolve,
  pkgAddresses: DirectDepRef[],
  workspaceRootDeps: HoistableRootDep[]
): (
  pair: Array<[string, MissingPeerInfo]>,
  alias: string,
  spec: string,
  declaringParents?: ReadonlyArray<{ name: string, version: string }>
) => boolean {
  const reachability = createLazyReachability(ctx, importer, pkgAddresses)
  return (missingRequiredPeers, alias, hoistedSpec, declaringParents = []) => {
    if (!isForbiddenManifestSpec(ctx, hoistedSpec)) return true
    const input: HoistedPeerCheckInput = { ctx, importer, missingRequiredPeers, pkgAddresses, workspaceRootDeps }
    if (hoistContextAuthorizesPeer(input, alias, hoistedSpec) || isAuthorizedLocalPeerSpec(ctx, reachability, alias, hoistedSpec)) {
      return true
    }
    return declaringParents.length > 0 &&
      declaringParents.every((parent) => candidateParentAuthorizesPeer(ctx, reachability, parent, alias, hoistedSpec))
  }
}

export function recordLocalPackagePeerSpecs (
  ctx: ResolutionContext,
  pkgId: PkgResolutionId,
  hooked: PackageManifest
): void {
  recordPkgNameVer(ctx, pkgId, hooked)
  let specs = ctx.localPkgPeerSpecsByPkgId.get(pkgId)
  for (const [alias, spec] of Object.entries(hooked.peerDependencies ?? {})) {
    if (typeof spec !== 'string' || !isForbiddenManifestSpec(ctx, spec)) continue
    if (specs == null) {
      specs = new Set<string>()
      ctx.localPkgPeerSpecsByPkgId.set(pkgId, specs)
    }
    specs.add(peerSpecKey(alias, spec))
  }
}

export function recordNonLocalPeerSpecs (
  ctx: ResolutionContext,
  pkgResponse: PackageResponse,
  preHook: DependencyManifest,
  hooked: PackageManifest
): void {
  const pkgId = pkgResponse.body.id
  recordPkgNameVer(ctx, pkgId, hooked)
  const parent: ParentIdentity = { name: hooked.name, version: hooked.version }
  for (const [alias, postSpec] of Object.entries(hooked.peerDependencies ?? {})) {
    if (typeof postSpec !== 'string' || !isForbiddenManifestSpec(ctx, postSpec)) continue
    const key = peerSpecKey(alias, postSpec)
    if (preHook.peerDependencies?.[alias] !== postSpec || overrideAuthorizesSpec(ctx.parsedOverrides, parent, alias, postSpec)) {
      addAuthorizedNonLocalPeer(ctx, pkgId, key)
    } else {
      addUntrustedNonLocalPeer(ctx, pkgId, key, describeNonLocalPackage(pkgId, hooked))
    }
  }
}

function recordPkgNameVer (ctx: ResolutionContext, pkgId: PkgResolutionId, manifest: PackageManifest): void {
  const identity = extractManifestNameVer(pkgId, manifest)
  if (identity != null) {
    ctx.pkgNameVerByPkgId.set(pkgId, identity)
  }
}

function extractManifestNameVer (
  pkgId: PkgResolutionId,
  manifest: PackageManifest
): { name: string, version: string } | undefined {
  if (manifest.name && manifest.version) {
    return { name: manifest.name, version: manifest.version }
  }
  const head = dp.removeSuffix(pkgId)
  const atIndex = head.lastIndexOf('@')
  if (atIndex < 1 || atIndex + 1 >= head.length) return undefined
  return { name: head.slice(0, atIndex), version: head.slice(atIndex + 1) }
}

function addAuthorizedNonLocalPeer (ctx: ResolutionContext, pkgId: PkgResolutionId, key: string): void {
  let set = ctx.authorizedNonLocalPeerSpecsByPkgId.get(pkgId)
  if (set == null) {
    set = new Set<string>()
    ctx.authorizedNonLocalPeerSpecsByPkgId.set(pkgId, set)
  }
  set.add(key)
}

function addUntrustedNonLocalPeer (
  ctx: ResolutionContext,
  pkgId: PkgResolutionId,
  key: string,
  parentLabel: string
): void {
  let map = ctx.untrustedLocalPeerParentsByPkgId.get(pkgId)
  if (map == null) {
    map = new Map<string, string>()
    ctx.untrustedLocalPeerParentsByPkgId.set(pkgId, map)
  }
  map.set(key, parentLabel)
}

function peerSpecKey (alias: string, spec: string): string {
  return `${alias}\0${spec}`
}

function findDisallowedHoistedPeerParent (
  input: HoistedPeerCheckInput,
  reachability: LazyReachability,
  alias: string,
  hoistedSpec: string
): string | undefined {
  const { ctx } = input
  if (!isForbiddenManifestSpec(ctx, hoistedSpec)) return undefined
  const parent = findUntrustedPeerParent(ctx, reachability, alias, hoistedSpec)
  if (parent == null) return undefined
  if (hoistContextAuthorizesPeer(input, alias, hoistedSpec)) return undefined
  return isAuthorizedLocalPeerSpec(ctx, reachability, alias, hoistedSpec) ? undefined : parent
}

function isAuthorizedLocalPeerSpec (
  ctx: ResolutionContext,
  reachability: LazyReachability,
  alias: string,
  spec: string
): boolean {
  return reachableLocalPkgDeclaresPeer(ctx, reachability.getReachablePkgIds(), alias, spec) ||
    overrideAuthorizesSpec(ctx.parsedOverrides, undefined, alias, spec) ||
    lockfileLocalPkgDeclaresPeer(ctx, reachability, alias, spec)
}

function reachableLocalPkgDeclaresPeer (
  ctx: ResolutionContext,
  reachablePkgIds: Set<PkgResolutionId>,
  alias: string,
  spec: string
): boolean {
  const key = peerSpecKey(alias, spec)
  for (const pkgId of reachablePkgIds) {
    if (ctx.localPkgPeerSpecsByPkgId.get(pkgId)?.has(key)) return true
  }
  return false
}

function candidateParentAuthorizesPeer (
  ctx: ResolutionContext,
  reachability: LazyReachability,
  parent: { name: string, version: string },
  alias: string,
  spec: string
): boolean {
  if (overrideAuthorizesSpec(ctx.parsedOverrides, parent, alias, spec)) return true
  const key = peerSpecKey(alias, spec)
  return inMemoryParentAuthorizesPeer(ctx, parent, key) ||
    lockfileLocalParentDeclaresPeer(ctx, reachability, parent, alias, spec)
}

function inMemoryParentAuthorizesPeer (
  ctx: ResolutionContext,
  parent: { name: string, version: string },
  key: string
): boolean {
  let foundAuthorized = false
  for (const [pkgId, info] of ctx.pkgNameVerByPkgId) {
    if (info.name !== parent.name || info.version !== parent.version) continue
    if (ctx.untrustedLocalPeerParentsByPkgId.get(pkgId)?.has(key)) return false
    if (ctx.authorizedNonLocalPeerSpecsByPkgId.get(pkgId)?.has(key) || ctx.localPkgPeerSpecsByPkgId.get(pkgId)?.has(key)) {
      foundAuthorized = true
    }
  }
  return foundAuthorized
}

function lockfileLocalParentDeclaresPeer (
  ctx: ResolutionContext,
  reachability: LazyReachability,
  parent: { name: string, version: string },
  alias: string,
  spec: string
): boolean {
  const packages = ctx.wantedLockfile.packages
  if (packages == null) return false
  const reachableLocal = reachability.getLockfileReachable(true)
  if (reachableLocal.size === 0) return false
  for (const [depPath, snapshot] of Object.entries(packages) as Array<[DepPath, PackageSnapshot]>) {
    if (!reachableLocal.has(dp.removeSuffix(depPath))) continue
    const parsed = dp.parse(depPath)
    const name = parsed.name ?? snapshot.name
    const version = parsed.version ?? snapshot.version
    if (name === parent.name && version === parent.version && snapshot.peerDependencies?.[alias] === spec) {
      return true
    }
  }
  return false
}

function findUntrustedPeerParent (
  ctx: ResolutionContext,
  reachability: LazyReachability,
  alias: string,
  spec: string
): string | undefined {
  const key = peerSpecKey(alias, spec)
  for (const pkgId of reachability.getReachablePkgIds()) {
    const parent = ctx.untrustedLocalPeerParentsByPkgId.get(pkgId)?.get(key)
    if (parent != null) return parent
  }
  return findLockfileUntrustedPeerParent(ctx, reachability, alias, spec)
}

function findLockfileUntrustedPeerParent (
  ctx: ResolutionContext,
  reachability: LazyReachability,
  alias: string,
  spec: string
): string | undefined {
  const packages = ctx.wantedLockfile.packages
  if (packages == null) return undefined
  const reachable = reachability.getLockfileReachable(false)
  const key = peerSpecKey(alias, spec)
  for (const [depPath, snapshot] of Object.entries(packages) as Array<[DepPath, PackageSnapshot]>) {
    const parsed = dp.parse(depPath)
    if (parsed.nonSemverVersion?.startsWith('file:')) continue
    const baseKey = dp.removeSuffix(depPath)
    if (!reachable.has(baseKey) || snapshot.peerDependencies?.[alias] !== spec) continue
    if (isAuthorizedLockfileNonLocalParent(ctx, { baseKey, parsed, snapshot, alias, spec, key })) continue
    return depPath
  }
  return undefined
}

function isAuthorizedLockfileNonLocalParent (
  ctx: ResolutionContext,
  { baseKey, parsed, snapshot, alias, spec, key }: {
    baseKey: string
    parsed: ReturnType<typeof dp.parse>
    snapshot: PackageSnapshot
    alias: string
    spec: string
    key: string
  }
): boolean {
  // A git or tarball parent is recorded under its bare source, which its
  // lockfile key prefixes with the package name.
  const resolverIds = [baseKey, parsed.version == null ? parsed.nonSemverVersion : undefined]
  if (resolverIds.some((pkgId) => pkgId != null && ctx.authorizedNonLocalPeerSpecsByPkgId.get(pkgId as PkgResolutionId)?.has(key))) {
    return true
  }
  const parent: ParentIdentity = {
    name: parsed.name ?? snapshot.name,
    version: parsed.version ?? snapshot.version,
  }
  return overrideAuthorizesSpec(ctx.parsedOverrides, parent, alias, spec)
}

function hoistContextAuthorizesPeer (
  { ctx, importer, missingRequiredPeers, workspaceRootDeps }: HoistedPeerCheckInput,
  alias: string,
  hoistedSpec: string
): boolean {
  const matchesRoot = workspaceRootDeps.some(
    (dep) => (dep.alias === alias || dep.pkgName === alias) && dep.normalizedBareSpecifier === hoistedSpec
  )
  if (matchesRoot || ctx.overrideBareSpecifier == null) return matchesRoot
  const range = missingRequiredPeers.find(([name]) => name === alias)?.[1].range ?? hoistedSpec
  return ctx.overrideBareSpecifier(alias, range, importer.options.prefix) === hoistedSpec
}

function lockfileLocalPkgDeclaresPeer (
  ctx: ResolutionContext,
  reachability: LazyReachability,
  alias: string,
  spec: string
): boolean {
  const packages = ctx.wantedLockfile.packages
  if (packages == null) return false
  const reachable = reachability.getLockfileReachable(true)
  if (reachable.size === 0) return false
  for (const [depPath, snapshot] of Object.entries(packages) as Array<[DepPath, PackageSnapshot]>) {
    if (reachable.has(dp.removeSuffix(depPath)) && snapshot.peerDependencies?.[alias] === spec) {
      return true
    }
  }
  return false
}
