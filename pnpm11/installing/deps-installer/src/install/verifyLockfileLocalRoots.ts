import * as dp from '@pnpm/deps.path'
import type { LockfileObject, PackageSnapshot, ProjectSnapshot } from '@pnpm/lockfile.types'
import { isTarballFilename } from '@pnpm/resolving.local-resolver'
import type { ResolutionPolicyViolation, WorkspacePackages } from '@pnpm/resolving.resolver-base'
import type { DepPath } from '@pnpm/types'

import { buildImporterScope } from './verifyLockfileImporterSeed.js'
import type { InstallLocalPolicyOptions } from './verifyLockfileLocalHooks.js'
import {
  anchorImporterLink,
  isNonRelativeTarget,
  type LocalResolutionPolicy,
  normalizeRelDir,
  targetEscapesWorkspace,
} from './verifyLockfileLocalPolicy.js'
import { isLocalGitPackage, isPurelyLocalFilesystemResolution } from './verifyLockfileShape.js'

export function withContextLocalPolicyOptions (
  opts: InstallLocalPolicyOptions & { ignorePackageManifest?: boolean },
  workspacePackages: WorkspacePackages | undefined,
  projects: Array<{ id: string }>
): InstallLocalPolicyOptions {
  return {
    ...opts,
    allProjects: opts.ignorePackageManifest ? undefined : opts.allProjects,
    workspacePackages: opts.ignorePackageManifest ? undefined : (workspacePackages ?? opts.workspacePackages),
    selectedImporterIds: opts.ignorePackageManifest ? undefined : projects.map((project) => project.id),
  }
}

export interface IndexedSnapshot {
  depPath: DepPath
  nonSemverVersion?: string
  snapshot: PackageSnapshot
}

export interface ImporterScope {
  reachableSnapshots: Set<DepPath>
  reachableBaseKeys: Set<string>
  localPackages: Set<string>
  localGitPackages: Set<string>
  linksByAlias: Set<string>
  violations: ResolutionPolicyViolation[]
}

export interface AuthorizedLocalRoots {
  workspaceDirs: Set<string>
  importers: ImporterScope[]
  snapshotsByBase: Map<string, IndexedSnapshot[]>
}

export function aliasLinkKey (alias: string, normalizedTarget: string): string {
  return `${alias}\0${normalizedTarget}`
}

export function isSnapshotBackedByLocalResolution (
  nonSemverVersion: string | undefined,
  snapshot: PackageSnapshot | undefined
): boolean {
  if (!nonSemverVersion?.startsWith('file:')) return false
  const resolution = snapshot?.resolution as { type?: unknown, tarball?: unknown } | undefined
  if (resolution == null) return true
  if (isPurelyLocalFilesystemResolution(resolution)) return true
  return isTarballFilename(nonSemverVersion.slice(5)) && resolution.type == null && resolution.tarball == null
}

export function indexSnapshotsByBase (lockfile: LockfileObject): Map<string, IndexedSnapshot[]> {
  const byBase = new Map<string, IndexedSnapshot[]>()
  for (const [depPath, snapshot] of Object.entries(lockfile.packages ?? {}) as Array<[DepPath, PackageSnapshot]>) {
    const parsed = dp.parse(depPath)
    const baseKey = dp.removeSuffix(depPath)
    const entry: IndexedSnapshot = { depPath, nonSemverVersion: parsed.nonSemverVersion, snapshot }
    const list = byBase.get(baseKey)
    if (list != null) list.push(entry)
    else byBase.set(baseKey, [entry])
  }
  return byBase
}

export function collectAuthorizedLocalRoots (
  lockfile: LockfileObject,
  policy: LocalResolutionPolicy
): AuthorizedLocalRoots {
  const workspaceDirs = collectWorkspaceImporterDirs(lockfile, policy)
  const lockfileImporterDirs = collectLockfileImporterDirs(lockfile)
  const snapshotsByBase = indexSnapshotsByBase(lockfile)
  const importers = collectActiveImporters(lockfile, policy, workspaceDirs)
    .map(([importerId, impSnap]) => buildImporterScope(
      { lockfile, policy, snapshotsByBase, workspaceDirs, lockfileImporterDirs, importerId },
      impSnap
    ))
  return { workspaceDirs, importers, snapshotsByBase }
}

function collectActiveImporters (
  lockfile: LockfileObject,
  policy: LocalResolutionPolicy,
  workspaceDirs: Set<string>
): Array<[string, ProjectSnapshot]> {
  const selected = policy.selectedImporterIds
  const restrictLive = policy.liveWorkspaceDirs != null
  const out: Array<[string, ProjectSnapshot]> = []
  for (const [importerId, importer] of Object.entries(lockfile.importers ?? {})) {
    const normId = normalizeRelDir(importerId)
    if (selected != null) {
      if (selected.has(normId)) out.push([normId, importer])
    } else if (!restrictLive || workspaceDirs.has(normId)) {
      out.push([normId, importer])
    }
  }
  return out
}

export function walkFullSnapshotClosure (
  lockfile: LockfileObject,
  seed: DepPath[],
  reachableBaseKeys: Set<string>
): Set<DepPath> {
  const visited = new Set<DepPath>(seed)
  const packages = lockfile.packages
  if (packages == null) return visited
  const queue = [...seed]
  for (let current = queue.pop(); current != null; current = queue.pop()) {
    const snapshot = packages[current]
    if (snapshot == null) continue
    enqueueFullDepMap(snapshot.dependencies, visited, reachableBaseKeys, queue)
    enqueueFullDepMap(snapshot.optionalDependencies, visited, reachableBaseKeys, queue)
  }
  return visited
}

function enqueueFullDepMap (
  deps: Record<string, string> | undefined,
  visited: Set<DepPath>,
  reachableBaseKeys: Set<string>,
  queue: DepPath[]
): void {
  if (deps == null) return
  for (const [alias, ref] of Object.entries(deps)) {
    const childDepPath = dp.refToRelative(ref, alias)
    if (childDepPath == null) continue
    reachableBaseKeys.add(dp.removeSuffix(childDepPath))
    if (!visited.has(childDepPath)) {
      visited.add(childDepPath)
      queue.push(childDepPath)
    }
  }
}

export function expandLocalSnapshotClosure (
  lockfile: LockfileObject,
  snapshotsByBase: Map<string, IndexedSnapshot[]>,
  seeds: { localSeed: DepPath[], localGitSeed: DepPath[] },
  out: { localPackages: Set<string>, localGitPackages: Set<string>, linksByAlias: Set<string> }
): void {
  const packages = lockfile.packages
  if (packages == null) return
  const visitedLocal = new Set<DepPath>(seeds.localSeed)
  const visitedGit = new Set<DepPath>(seeds.localGitSeed)
  const localQueue = [...seeds.localSeed]
  const gitQueue = [...seeds.localGitSeed]
  for (let current = localQueue.pop(); current != null; current = localQueue.pop()) {
    const snapshot = packages[current]
    if (snapshot == null) continue
    for (const deps of [snapshot.dependencies, snapshot.optionalDependencies]) {
      enqueueLocalDepMap({ lockfile, snapshotsByBase, deps }, { ...out, visitedLocal, visitedGit, localQueue, gitQueue })
    }
  }
  expandLocalGitQueue(lockfile, snapshotsByBase, visitedGit, gitQueue, out.localGitPackages)
}

function expandLocalGitQueue (
  lockfile: LockfileObject,
  snapshotsByBase: Map<string, IndexedSnapshot[]>,
  visitedGit: Set<DepPath>,
  gitQueue: DepPath[],
  localGitPackages: Set<string>
): void {
  const packages = lockfile.packages
  if (packages == null) return
  for (let current = gitQueue.pop(); current != null; current = gitQueue.pop()) {
    const snapshot = packages[current]
    if (snapshot == null) continue
    enqueueLocalGitDeps({ packages, snapshotsByBase, deps: snapshot.dependencies }, { visitedGit, gitQueue, localGitPackages })
    enqueueLocalGitDeps({ packages, snapshotsByBase, deps: snapshot.optionalDependencies }, { visitedGit, gitQueue, localGitPackages })
  }
}

function enqueueLocalGitDeps (
  { packages, snapshotsByBase, deps }: {
    packages: Record<DepPath, PackageSnapshot>
    snapshotsByBase: Map<string, IndexedSnapshot[]>
    deps: Record<string, string> | undefined
  },
  acc: { visitedGit: Set<DepPath>, gitQueue: DepPath[], localGitPackages: Set<string> }
): void {
  if (deps == null) return
  for (const [alias, ref] of Object.entries(deps)) {
    const childDepPath = dp.refToRelative(ref, alias)
    if (childDepPath == null) continue
    const parsed = dp.parse(childDepPath)
    const baseKey = dp.removeSuffix(childDepPath)
    const snap = packages[childDepPath] ?? snapshotsByBase.get(baseKey)?.[0]?.snapshot
    if (!isLocalGitPackage(parsed.nonSemverVersion, snap?.resolution)) continue
    acc.localGitPackages.add(baseKey)
    if (!acc.visitedGit.has(childDepPath)) {
      acc.visitedGit.add(childDepPath)
      acc.gitQueue.push(childDepPath)
    }
  }
}

function enqueueLocalDepMap (
  ctx: {
    lockfile: LockfileObject
    snapshotsByBase: Map<string, IndexedSnapshot[]>
    deps: Record<string, string> | undefined
  },
  acc: {
    localPackages: Set<string>
    localGitPackages: Set<string>
    linksByAlias: Set<string>
    visitedLocal: Set<DepPath>
    visitedGit: Set<DepPath>
    localQueue: DepPath[]
    gitQueue: DepPath[]
  }
): void {
  if (ctx.deps == null) return
  for (const [alias, ref] of Object.entries(ctx.deps)) {
    enqueueSingleLocalDep(ctx, alias, ref, acc)
  }
}

function enqueueSingleLocalDep (
  { lockfile, snapshotsByBase }: { lockfile: LockfileObject, snapshotsByBase: Map<string, IndexedSnapshot[]> },
  alias: string,
  ref: string,
  acc: {
    localPackages: Set<string>
    localGitPackages: Set<string>
    linksByAlias: Set<string>
    visitedLocal: Set<DepPath>
    visitedGit: Set<DepPath>
    localQueue: DepPath[]
    gitQueue: DepPath[]
  }
): void {
  if (ref.startsWith('link:')) {
    acc.linksByAlias.add(aliasLinkKey(alias, normalizeRelDir(ref.slice(5))))
    return
  }
  const childDepPath = dp.refToRelative(ref, alias)
  if (childDepPath == null) return
  const parsed = dp.parse(childDepPath)
  const baseKey = dp.removeSuffix(childDepPath)
  const snap = lockfile.packages?.[childDepPath] ?? snapshotsByBase.get(baseKey)?.[0]?.snapshot
  if (isSnapshotBackedByLocalResolution(parsed.nonSemverVersion, snap)) {
    acc.localPackages.add(baseKey)
    pushUnvisitedDep(childDepPath, acc.visitedLocal, acc.localQueue)
  } else if (isLocalGitPackage(parsed.nonSemverVersion, snap?.resolution)) {
    acc.localGitPackages.add(baseKey)
    pushUnvisitedDep(childDepPath, acc.visitedGit, acc.gitQueue)
  }
}

function pushUnvisitedDep<QueueKey extends string> (item: QueueKey, visited: Set<QueueKey>, queue: QueueKey[]): void {
  if (!visited.has(item)) {
    visited.add(item)
    queue.push(item)
  }
}

export function enqueueReachablePolicyLocalChildren (
  ctx: {
    lockfile: LockfileObject
    snapshotsByBase: Map<string, IndexedSnapshot[]>
    deps: Record<string, string> | undefined
    reachableSnapshots: Set<DepPath>
  },
  out: {
    authorizedLocal: Set<string>
    authorizedGit: Set<string>
    linksByAlias: Set<string>
    localQueue: string[]
    gitQueue: string[]
  }
): void {
  for (const [alias, ref] of Object.entries(ctx.deps ?? {})) {
    enqueueSingleReachablePolicyChild(ctx, alias, ref, out)
  }
}

function enqueueSingleReachablePolicyChild (
  { lockfile, snapshotsByBase, reachableSnapshots }: {
    lockfile: LockfileObject
    snapshotsByBase: Map<string, IndexedSnapshot[]>
    reachableSnapshots: Set<DepPath>
  },
  alias: string,
  ref: string,
  out: {
    authorizedLocal: Set<string>
    authorizedGit: Set<string>
    linksByAlias: Set<string>
    localQueue: string[]
    gitQueue: string[]
  }
): void {
  if (ref.startsWith('link:')) {
    out.linksByAlias.add(aliasLinkKey(alias, normalizeRelDir(ref.slice(5))))
    return
  }
  const childDepPath = dp.refToRelative(ref, alias)
  if (childDepPath == null || !reachableSnapshots.has(childDepPath)) return
  const parsed = dp.parse(childDepPath)
  const baseKey = dp.removeSuffix(childDepPath)
  const snap = lockfile.packages?.[childDepPath] ?? snapshotsByBase.get(baseKey)?.[0]?.snapshot
  if (isSnapshotBackedByLocalResolution(parsed.nonSemverVersion, snap)) {
    pushUnvisitedDep(baseKey, out.authorizedLocal, out.localQueue)
  } else if (isLocalGitPackage(parsed.nonSemverVersion, snap?.resolution)) {
    pushUnvisitedDep(baseKey, out.authorizedGit, out.gitQueue)
  }
}

/**
 * Relative directories of every importer recorded in the lockfile, live or not.
 */
function collectLockfileImporterDirs (lockfile: LockfileObject): Set<string> {
  const dirs = new Set<string>()
  for (const importerId of Object.keys(lockfile.importers ?? {})) {
    if (!isNonRelativeTarget(importerId)) dirs.add(normalizeRelDir(importerId))
  }
  return dirs
}

function collectWorkspaceImporterDirs (lockfile: LockfileObject, policy: LocalResolutionPolicy): Set<string> {
  if (policy.liveWorkspaceDirs != null) {
    const dirs = new Set<string>(policy.liveWorkspaceDirs)
    for (const extra of policy.extraWorkspaceDirs ?? []) dirs.add(extra)
    return dirs
  }
  const dirs = new Set<string>(policy.extraWorkspaceDirs)
  for (const [importerId, importer] of Object.entries(lockfile.importers ?? {})) {
    addContainedImporterDirs(policy, importerId, importer.publishDirectory, dirs)
  }
  return dirs
}

function addContainedImporterDirs (
  policy: LocalResolutionPolicy,
  importerId: string,
  publishDirectory: string | undefined,
  dirs: Set<string>
): void {
  if (isNonRelativeTarget(importerId) || targetEscapesWorkspace(policy, importerId)) return
  const normalizedId = normalizeRelDir(importerId)
  dirs.add(normalizedId)
  if (!publishDirectory) return
  const combined = anchorImporterLink(normalizedId, publishDirectory)
  if (!isNonRelativeTarget(combined) && !targetEscapesWorkspace(policy, combined)) {
    dirs.add(combined)
  }
}
