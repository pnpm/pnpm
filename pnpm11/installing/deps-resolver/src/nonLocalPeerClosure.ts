import * as dp from '@pnpm/deps.path'
import type { LockfileObject, PackageSnapshot } from '@pnpm/lockfile.types'
import { isHostFilesystemSpecifier, isNonLocalFilesystemSubdepSpec } from '@pnpm/resolving.local-resolver'
import type { PkgResolutionId } from '@pnpm/resolving.resolver-base'
import type { DepPath, ProjectId } from '@pnpm/types'

import type { ImporterToResolve, PkgAddressOrLink, ResolutionContext, ResolvedPackage } from './resolutionTypes.js'

export type DirectDepRef = Pick<PkgAddressOrLink, 'alias' | 'pkgId'> & {
  isLinkedDependency?: boolean
  depIsLinked?: boolean
  resolvedVia?: string
}

export interface LazyReachability {
  importerId: ProjectId
  getReachablePkgIds: () => Set<PkgResolutionId>
  getLockfileReachable: (localOnly: boolean) => Set<string>
}

export function createLazyReachability (
  ctx: ResolutionContext,
  importer: ImporterToResolve,
  pkgAddresses: DirectDepRef[]
): LazyReachability {
  const importerId = importer.options.parentIds[0] as string as ProjectId
  let reachablePkgIds: Set<PkgResolutionId> | undefined
  let lockfileLocal: Set<string> | undefined
  let lockfileAll: Set<string> | undefined
  let directAliases: Set<string> | undefined
  let localDirectAliases: Set<string> | undefined
  const getDirectAliases = (): Set<string> => {
    directAliases ??= collectCurrentDirectAliases(importer, pkgAddresses)
    return directAliases
  }
  const getLocalDirectAliases = (): Set<string> => {
    localDirectAliases ??= collectCurrentLocalDirectAliases(ctx, importer, pkgAddresses)
    return localDirectAliases
  }
  return {
    importerId,
    getReachablePkgIds: () => {
      reachablePkgIds ??= collectImporterReachablePkgIds(ctx, pkgAddresses)
      return reachablePkgIds
    },
    getLockfileReachable: (localOnly: boolean) => {
      if (localOnly) {
        lockfileLocal ??= collectImporterLockfileBaseKeys(ctx.wantedLockfile, importerId, getLocalDirectAliases(), true)
        return lockfileLocal
      }
      lockfileAll ??= collectImporterLockfileBaseKeys(ctx.wantedLockfile, importerId, getDirectAliases(), false)
      return lockfileAll
    },
  }
}

function collectCurrentDirectAliases (
  importer: ImporterToResolve,
  pkgAddresses: DirectDepRef[]
): Set<string> {
  const aliases = new Set<string>()
  for (const addr of pkgAddresses) {
    if (addr.alias) aliases.add(addr.alias)
  }
  for (const wanted of importer.wantedDependencies) {
    if (wanted.alias) aliases.add(wanted.alias)
  }
  return aliases
}

function collectCurrentLocalDirectAliases (
  ctx: ResolutionContext,
  importer: ImporterToResolve,
  pkgAddresses: DirectDepRef[]
): Set<string> {
  const localAliases = new Set<string>()
  const seenResolved = new Set<string>()
  for (const addr of pkgAddresses) {
    if (!addr.alias) continue
    seenResolved.add(addr.alias)
    if (isLocalDirectDep(addr, ctx.resolvedPkgsById[addr.pkgId])) localAliases.add(addr.alias)
  }
  addUnresolvedLocalWantedAliases(importer.wantedDependencies, seenResolved, localAliases)
  return localAliases
}

function addUnresolvedLocalWantedAliases (
  wantedDependencies: ImporterToResolve['wantedDependencies'],
  seenResolved: Set<string>,
  out: Set<string>
): void {
  for (const wanted of wantedDependencies) {
    if (wanted.alias && !seenResolved.has(wanted.alias) && isLocalOrWorkspaceBareSpec(wanted.bareSpecifier)) {
      out.add(wanted.alias)
    }
  }
}

function isLocalDirectDep (addr: DirectDepRef, resolved: ResolvedPackage | undefined): boolean {
  if (addr.isLinkedDependency || addr.depIsLinked) return true
  const rawId = addr.pkgId
  if (dp.packageRootLinkTarget(rawId) != null) return false
  if (
    rawId.startsWith('file:') ||
    rawId.startsWith('link:') ||
    rawId.includes('@file:') ||
    rawId.includes('@link:')
  ) {
    return true
  }
  const via = addr.resolvedVia ?? resolved?.resolvedVia
  return via === 'local-filesystem' || via === 'workspace' || resolved?.resolution.type === 'directory'
}

function isLocalOrWorkspaceBareSpec (bareSpecifier: string | undefined): boolean {
  if (!bareSpecifier) return false
  const trimmed = bareSpecifier.trim()
  return trimmed.slice(0, 10).toLowerCase() === 'workspace:' ||
    isNonLocalFilesystemSubdepSpec(trimmed, () => false) ||
    isHostFilesystemSpecifier(trimmed, '.', () => false)
}

function collectImporterReachablePkgIds (
  ctx: ResolutionContext,
  roots: Array<Pick<DirectDepRef, 'pkgId'>>
): Set<PkgResolutionId> {
  const visited = new Set<PkgResolutionId>()
  const pending = roots.map(({ pkgId }) => pkgId)
  while (pending.length > 0) {
    const pkgId = pending.pop()!
    if (visited.has(pkgId)) continue
    visited.add(pkgId)
    for (const child of ctx.childrenByParentId[pkgId] ?? []) {
      pending.push(child.id)
    }
  }
  return visited
}

function collectImporterLockfileBaseKeys (
  lockfile: LockfileObject,
  importerId: ProjectId,
  currentDirectAliases: Set<string>,
  localOnly: boolean
): Set<string> {
  const visited = seedSingleImporterFullKeys(lockfile, importerId, currentDirectAliases, localOnly)
  const packages = lockfile.packages
  if (packages != null) {
    seedImporterSelfDirectoryKey(packages, importerId, visited)
    const queue = Array.from(visited)
    for (let current = queue.pop(); current != null; current = queue.pop()) {
      const snapshot = packages[current]
      if (snapshot == null) continue
      enqueueSnapshotClosureDeps(snapshot.dependencies, localOnly, visited, queue)
      enqueueSnapshotClosureDeps(snapshot.optionalDependencies, localOnly, visited, queue)
    }
  }
  const baseKeys = new Set<string>()
  for (const depPath of visited) baseKeys.add(dp.removeSuffix(depPath))
  return baseKeys
}

function seedSingleImporterFullKeys (
  lockfile: LockfileObject,
  importerId: ProjectId,
  currentDirectAliases: Set<string>,
  localOnly: boolean
): Set<DepPath> {
  const visited = new Set<DepPath>()
  const importer = lockfile.importers?.[importerId]
  if (importer == null) return visited
  for (const depMap of [importer.dependencies, importer.devDependencies, importer.optionalDependencies]) {
    pushImporterFullKeys(depMap, currentDirectAliases, localOnly, visited)
  }
  return visited
}

function seedImporterSelfDirectoryKey (
  packages: Record<DepPath, PackageSnapshot>,
  importerId: ProjectId,
  visited: Set<DepPath>
): void {
  for (const [depPath, snapshot] of Object.entries(packages) as Array<[DepPath, PackageSnapshot]>) {
    const isFile = Boolean(dp.parse(depPath).nonSemverVersion?.startsWith('file:'))
    const dir = (snapshot.resolution as { directory?: unknown } | undefined)?.directory
    if (isFile && dir === importerId) visited.add(depPath)
  }
}

function pushImporterFullKeys (
  depMap: Record<string, string> | undefined,
  currentDirectAliases: Set<string>,
  localOnly: boolean,
  out: Set<DepPath>
): void {
  if (depMap == null) return
  for (const [alias, ref] of Object.entries(depMap)) {
    if (!currentDirectAliases.has(alias)) continue
    const childPath = dp.refToRelative(ref, alias)
    if (childPath == null) continue
    if (!localOnly || dp.parse(childPath).nonSemverVersion?.startsWith('file:')) {
      out.add(childPath)
    }
  }
}

function enqueueSnapshotClosureDeps (
  deps: Record<string, string> | undefined,
  localOnly: boolean,
  visited: Set<DepPath>,
  queue: DepPath[]
): void {
  if (deps == null) return
  for (const [alias, ref] of Object.entries(deps)) {
    const childPath = dp.refToRelative(ref, alias)
    if (childPath == null) continue
    if (localOnly && !dp.parse(childPath).nonSemverVersion?.startsWith('file:')) continue
    if (!visited.has(childPath)) {
      visited.add(childPath)
      queue.push(childPath)
    }
  }
}
