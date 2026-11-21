import * as dp from '@pnpm/deps.path'
import type { LockfileObject, PackageSnapshot } from '@pnpm/lockfile.types'
import type { ResolutionPolicyViolation } from '@pnpm/resolving.resolver-base'
import type { DepPath } from '@pnpm/types'

import { lockfileHasAnyLocalSurface } from './verifyLockfileFastScan.js'
import {
  type LocalDepField,
  type LocalResolutionPolicy,
  normalizeRelDir,
  policyAuthorizesLink,
  policyAuthorizesPackage,
  policyHasAuthorizationSource,
} from './verifyLockfileLocalPolicy.js'
import {
  aliasLinkKey,
  type AuthorizedLocalRoots,
  collectAuthorizedLocalRoots,
  enqueueReachablePolicyLocalChildren,
  type ImporterScope,
  type IndexedSnapshot,
  isSnapshotBackedByLocalResolution,
} from './verifyLockfileLocalRoots.js'
import {
  collectPackageEntryViolations,
  isAllowedWorkspaceTarget,
  isDeploySnapshot,
  isWorkspaceImporterDirectory,
  packageViolation,
} from './verifyLockfilePackageEntries.js'
import {
  hasLocalFilesystemResolution,
  isLocalGitPackage,
} from './verifyLockfileShape.js'

export { type InstallLocalPolicyOptions, localPolicyForInstallOptions, resolveLocalPolicyForInstall } from './verifyLockfileLocalHooks.js'
export {
  type AuthorizedLocalEdge, computeWorkspaceDepth, createLocalResolutionPolicy,
  type LiveWorkspaceProject, type LocalHookInputs, type LocalResolutionPolicy,
  policyHasChildRangedEdges, stripFileUrlPrefix,
} from './verifyLockfileLocalPolicy.js'
export { withContextLocalPolicyOptions } from './verifyLockfileLocalRoots.js'
export { fileKeyMismatchesLocalTarget, hasLocalFilesystemResolution, isDeployFileUrlKey, isPurelyLocalFilesystemResolution } from './verifyLockfileShape.js'

export const LOCAL_SUBDEP_NOT_ALLOWED_VIOLATION_CODE = 'LOCAL_SUBDEP_NOT_ALLOWED'
const CHILD_DEP_FIELDS = ['dependencies', 'optionalDependencies'] as const

export function collectLocalResolutionViolations (
  lockfile: LockfileObject,
  localPolicy?: LocalResolutionPolicy
): ResolutionPolicyViolation[] {
  if (!lockfileHasAnyLocalSurface(lockfile)) return []
  const policy: LocalResolutionPolicy = localPolicy ?? { workspaceDepthAboveLockfile: 0 }
  const roots = collectAuthorizedLocalRoots(lockfile, policy)
  const visitedSnapshots = new Set<DepPath>()
  const reachableBaseKeys = new Set<string>()
  const authorizedPackages = new Set<string>()
  const authorizedGitPackages = new Set<string>()
  const rawViolations: ResolutionPolicyViolation[] = []
  for (const imp of roots.importers) {
    rawViolations.push(...imp.violations)
    for (const key of imp.reachableBaseKeys) reachableBaseKeys.add(key)
    for (const key of imp.localPackages) authorizedPackages.add(key)
    for (const key of imp.localGitPackages) authorizedGitPackages.add(key)
    scanImporterScope(lockfile, policy, roots, imp, {
      visitedSnapshots,
      authorizedPackages,
      authorizedGitPackages,
      violations: rawViolations,
    })
  }
  if (policy.selectedImporterIds == null) {
    scanUnreachableSnapshots(lockfile, policy, roots, visitedSnapshots, rawViolations)
  }
  const reachableFilter = policy.selectedImporterIds != null ? reachableBaseKeys : undefined
  collectPackageEntryViolations({
    lockfile,
    policy,
    workspaceDirs: roots.workspaceDirs,
    authorizedPackages,
    authorizedGitPackages,
    reachableFilter,
  }, rawViolations)
  return dedupeViolations(rawViolations)
}

interface ScopedScan {
  lockfile: LockfileObject
  policy: LocalResolutionPolicy
  workspaceDirs: Set<string>
  snapshotsByBase: Map<string, IndexedSnapshot[]>
  localPackages: Set<string>
  localGitPackages: Set<string>
  linksByAlias: Set<string>
  policyPackages: Set<string>
  policyGitPackages: Set<string>
}

interface ParentContext {
  baseKey: string
  name: string
  version?: string
  displayVersion: string
  snapshot: PackageSnapshot
  isAuthorizedGit: boolean
}

function scanImporterScope (
  lockfile: LockfileObject,
  policy: LocalResolutionPolicy,
  roots: AuthorizedLocalRoots,
  imp: ImporterScope,
  acc: {
    visitedSnapshots: Set<DepPath>
    authorizedPackages: Set<string>
    authorizedGitPackages: Set<string>
    violations: ResolutionPolicyViolation[]
  }
): void {
  const packages = lockfile.packages
  if (packages == null) return
  const scan: ScopedScan = {
    lockfile,
    policy,
    workspaceDirs: roots.workspaceDirs,
    snapshotsByBase: roots.snapshotsByBase,
    localPackages: imp.localPackages,
    localGitPackages: imp.localGitPackages,
    linksByAlias: new Set<string>(imp.linksByAlias),
    policyPackages: new Set<string>(),
    policyGitPackages: new Set<string>(),
  }
  if (policyHasAuthorizationSource(policy)) {
    seedReachablePolicyClosures(scan, packages, imp.reachableSnapshots)
    for (const key of scan.policyPackages) acc.authorizedPackages.add(key)
    for (const key of scan.policyGitPackages) acc.authorizedGitPackages.add(key)
  }
  for (const depPath of imp.reachableSnapshots) {
    acc.visitedSnapshots.add(depPath)
    const snapshot = packages[depPath]
    if (snapshot != null) inspectSnapshotEntry(scan, depPath, snapshot, acc.violations)
  }
}

function scanUnreachableSnapshots (
  lockfile: LockfileObject,
  policy: LocalResolutionPolicy,
  roots: AuthorizedLocalRoots,
  visitedSnapshots: Set<DepPath>,
  violations: ResolutionPolicyViolation[]
): void {
  const scan: ScopedScan = {
    lockfile,
    policy,
    workspaceDirs: roots.workspaceDirs,
    snapshotsByBase: roots.snapshotsByBase,
    localPackages: new Set<string>(),
    localGitPackages: new Set<string>(),
    linksByAlias: new Set<string>(),
    policyPackages: new Set<string>(),
    policyGitPackages: new Set<string>(),
  }
  for (const [depPath, snapshot] of Object.entries(lockfile.packages ?? {}) as Array<[DepPath, PackageSnapshot]>) {
    if (!visitedSnapshots.has(depPath)) inspectSnapshotEntry(scan, depPath, snapshot, violations)
  }
}

function seedReachablePolicyClosures (
  scan: ScopedScan,
  packages: Record<DepPath, PackageSnapshot>,
  reachableSnapshots: Set<DepPath>
): void {
  for (const depPath of reachableSnapshots) {
    const snapshot = packages[depPath]
    if (snapshot == null) continue
    const parent = buildParentContext(scan, depPath, snapshot)
    for (const field of CHILD_DEP_FIELDS) {
      for (const [alias, ref] of Object.entries(snapshot[field] ?? {})) {
        seedPolicyPackageChild(scan, reachableSnapshots, parent, { field, alias, ref })
      }
    }
  }
}

function seedPolicyPackageChild (
  scan: ScopedScan,
  reachableSnapshots: Set<DepPath>,
  parent: ParentContext,
  { field, alias, ref }: { field: LocalDepField, alias: string, ref: string }
): void {
  const resolved = classifyChildRef(scan, alias, ref)
  if (resolved == null) return
  if (policyAuthorizesPackage(scan.policy, scan.lockfile, parent, { field, alias, nonSemverVersion: resolved.nonSemverVersion, snapshot: resolved.snapshot })) {
    recordPolicyPackageClosure(scan, reachableSnapshots, resolved.baseKey, resolved.isLocalFs)
  }
}

function buildParentContext (scan: ScopedScan, depPath: DepPath, snapshot: PackageSnapshot): ParentContext {
  const parsedParent = dp.parse(depPath)
  const baseKey = dp.removeSuffix(depPath)
  const isAuthorizedGit = isLocalGitPackage(parsedParent.nonSemverVersion, snapshot.resolution) &&
    (scan.localGitPackages.has(baseKey) || scan.policyGitPackages.has(baseKey))
  return {
    baseKey,
    name: parsedParent.name ?? snapshot.name ?? depPath,
    version: parsedParent.version ?? snapshot.version,
    displayVersion: parsedParent.version ?? snapshot.version ?? parsedParent.nonSemverVersion ?? '0.0.0',
    snapshot,
    isAuthorizedGit,
  }
}

function inspectSnapshotEntry (
  scan: ScopedScan,
  depPath: DepPath,
  snapshot: PackageSnapshot,
  violations: ResolutionPolicyViolation[]
): void {
  const parsedParent = dp.parse(depPath)
  const baseSnap = dp.removeSuffix(depPath)
  if (
    isSnapshotBackedByLocalResolution(parsedParent.nonSemverVersion, snapshot) &&
    isAuthorizedLocalPackage(scan, baseSnap, parsedParent.nonSemverVersion, snapshot)
  ) {
    return
  }
  const parent = buildParentContext(scan, depPath, snapshot)
  for (const field of CHILD_DEP_FIELDS) {
    for (const [alias, ref] of Object.entries(snapshot[field] ?? {})) {
      inspectNonLocalChild(scan, parent, { field, alias, ref }, violations)
    }
  }
}

function inspectNonLocalChild (
  scan: ScopedScan,
  parent: ParentContext,
  child: { field: LocalDepField, alias: string, ref: string },
  violations: ResolutionPolicyViolation[]
): void {
  if (child.ref.startsWith('link:')) {
    if (!isAllowedSnapshotLink(scan, parent, child)) {
      violations.push(linkViolation(parent.name, parent.displayVersion, child.ref.slice(5), 'a non-local package snapshot links to an unauthorized host filesystem path'))
    }
    return
  }
  const resolved = classifyChildRef(scan, child.alias, child.ref)
  if (resolved == null || (resolved.isLocalGit && parent.isAuthorizedGit)) return
  if (allowNonLocalChildEdge(scan, parent, child.alias, { field: child.field, ...resolved })) return
  violations.push(packageViolation(parent.name, parent.displayVersion, resolved.snapshot, LOCAL_SUBDEP_NOT_ALLOWED_VIOLATION_CODE, 'a non-local package snapshot references an unauthorized local filesystem dependency'))
}

function classifyChildRef (
  scan: ScopedScan,
  alias: string,
  ref: string
): { baseKey: string, nonSemverVersion?: string, snapshot?: PackageSnapshot, isLocalFs: boolean, isLocalGit: boolean } | undefined {
  const childDepPath = dp.refToRelative(ref, alias)
  if (childDepPath == null) return undefined
  const parsedChild = dp.parse(childDepPath)
  const baseKey = dp.removeSuffix(childDepPath)
  const snapshot = findSnapshotByBaseKey(scan, childDepPath, baseKey)
  const isLocalFs = Boolean(parsedChild.nonSemverVersion?.startsWith('file:')) ||
    (snapshot != null && hasLocalFilesystemResolution(snapshot.resolution))
  const isLocalGit = !isLocalFs && isLocalGitPackage(parsedChild.nonSemverVersion, snapshot?.resolution)
  if (!isLocalFs && !isLocalGit) return undefined
  return { baseKey, nonSemverVersion: parsedChild.nonSemverVersion, snapshot, isLocalFs, isLocalGit }
}

function isAllowedSnapshotLink (
  scan: ScopedScan,
  parent: ParentContext,
  { field, alias, ref }: { field: LocalDepField, alias: string, ref: string }
): boolean {
  if (dp.packageRootLinkTarget(ref) != null) return true
  const target = ref.slice(5)
  if (target.startsWith('<root>')) return false
  if (policyAuthorizesLink(scan.policy, scan.lockfile, parent, { field, alias, target })) return true
  const key = aliasLinkKey(alias, normalizeRelDir(target))
  const peerAllowed = parentDeclaresPeer(parent.snapshot, alias) && scan.linksByAlias.has(key)
  return peerAllowed || isAllowedWorkspaceTarget(scan.workspaceDirs, target)
}

function allowNonLocalChildEdge (
  scan: ScopedScan,
  parent: ParentContext,
  alias: string,
  child: { field: LocalDepField, baseKey: string, nonSemverVersion?: string, snapshot?: PackageSnapshot, isLocalGit: boolean }
): boolean {
  const peerRootsMatch = child.isLocalGit
    ? (scan.localGitPackages.has(child.baseKey) || scan.policyGitPackages.has(child.baseKey))
    : (scan.localPackages.has(child.baseKey) || scan.policyPackages.has(child.baseKey))
  return isDeploySnapshot(scan.policy, child.nonSemverVersion, child.snapshot) ||
    isWorkspaceImporterDirectory(scan.policy, scan.workspaceDirs, child.nonSemverVersion, child.snapshot) ||
    (parentDeclaresPeer(parent.snapshot, alias) && peerRootsMatch) ||
    policyAuthorizesPackage(scan.policy, scan.lockfile, parent, { field: child.field, alias, nonSemverVersion: child.nonSemverVersion, snapshot: child.snapshot })
}

function recordPolicyPackageClosure (
  scan: ScopedScan,
  reachableSnapshots: Set<DepPath>,
  rootKey: string,
  isLocalFs: boolean
): void {
  const localQueue: string[] = []
  const gitQueue: string[] = []
  seedPolicyRootKey(scan, rootKey, isLocalFs, { localQueue, gitQueue })
  drainPolicyLocalQueue(scan, reachableSnapshots, { localQueue, gitQueue })
  expandPolicyGitClosure(scan, reachableSnapshots, gitQueue)
}

function seedPolicyRootKey (
  scan: ScopedScan,
  rootKey: string,
  isLocalFs: boolean,
  queues: { localQueue: string[], gitQueue: string[] }
): void {
  const targetSet = isLocalFs ? scan.policyPackages : scan.policyGitPackages
  const targetQueue = isLocalFs ? queues.localQueue : queues.gitQueue
  if (!targetSet.has(rootKey)) {
    targetSet.add(rootKey)
    targetQueue.push(rootKey)
  }
}

function drainPolicyLocalQueue (
  scan: ScopedScan,
  reachableSnapshots: Set<DepPath>,
  { localQueue, gitQueue }: { localQueue: string[], gitQueue: string[] }
): void {
  const sink = {
    authorizedLocal: scan.policyPackages,
    authorizedGit: scan.policyGitPackages,
    linksByAlias: scan.linksByAlias,
    localQueue,
    gitQueue,
  }
  for (let current = localQueue.pop(); current != null; current = localQueue.pop()) {
    for (const { depPath, nonSemverVersion, snapshot } of scan.snapshotsByBase.get(current) ?? []) {
      if (!reachableSnapshots.has(depPath) || !isSnapshotBackedByLocalResolution(nonSemverVersion, snapshot)) continue
      enqueueReachablePolicyLocalChildren({ lockfile: scan.lockfile, snapshotsByBase: scan.snapshotsByBase, deps: snapshot.dependencies, reachableSnapshots }, sink)
      enqueueReachablePolicyLocalChildren({ lockfile: scan.lockfile, snapshotsByBase: scan.snapshotsByBase, deps: snapshot.optionalDependencies, reachableSnapshots }, sink)
    }
  }
}

function expandPolicyGitClosure (scan: ScopedScan, reachableSnapshots: Set<DepPath>, gitQueue: string[]): void {
  for (let current = gitQueue.pop(); current != null; current = gitQueue.pop()) {
    for (const { depPath, snapshot } of scan.snapshotsByBase.get(current) ?? []) {
      if (!reachableSnapshots.has(depPath)) continue
      enqueueReachableGitDeps(scan, reachableSnapshots, snapshot.dependencies, gitQueue)
      enqueueReachableGitDeps(scan, reachableSnapshots, snapshot.optionalDependencies, gitQueue)
    }
  }
}

function enqueueReachableGitDeps (
  scan: ScopedScan,
  reachableSnapshots: Set<DepPath>,
  deps: Record<string, string> | undefined,
  gitQueue: string[]
): void {
  for (const [alias, ref] of Object.entries(deps ?? {})) {
    const childDepPath = dp.refToRelative(ref, alias)
    if (childDepPath == null || !reachableSnapshots.has(childDepPath)) continue
    const parsed = dp.parse(childDepPath)
    const baseKey = dp.removeSuffix(childDepPath)
    const snap = findSnapshotByBaseKey(scan, childDepPath, baseKey)
    if (isLocalGitPackage(parsed.nonSemverVersion, snap?.resolution) && !scan.policyGitPackages.has(baseKey)) {
      scan.policyGitPackages.add(baseKey)
      gitQueue.push(baseKey)
    }
  }
}

function isAuthorizedLocalPackage (
  scan: ScopedScan,
  baseKey: string,
  nonSemverVersion: string | undefined,
  snapshot: PackageSnapshot | undefined
): boolean {
  return isDeploySnapshot(scan.policy, nonSemverVersion, snapshot) ||
    scan.localPackages.has(baseKey) ||
    scan.policyPackages.has(baseKey) ||
    isWorkspaceImporterDirectory(scan.policy, scan.workspaceDirs, nonSemverVersion, snapshot)
}

function parentDeclaresPeer (snapshot: PackageSnapshot, alias: string): boolean {
  return Boolean(snapshot.peerDependencies?.[alias] != null || snapshot.peerDependenciesMeta?.[alias] != null)
}

function findSnapshotByBaseKey (scan: ScopedScan, depPath: DepPath, baseKey: string): PackageSnapshot | undefined {
  const packages = scan.lockfile.packages
  if (packages == null) return undefined
  return packages[depPath] ?? packages[baseKey as DepPath] ?? scan.snapshotsByBase.get(baseKey)?.[0]?.snapshot
}

function linkViolation (name: string, version: string, directory: string, reason: string): ResolutionPolicyViolation {
  return { name, version, resolution: { type: 'directory', directory }, code: LOCAL_SUBDEP_NOT_ALLOWED_VIOLATION_CODE, reason }
}

function dedupeViolations (raw: ResolutionPolicyViolation[]): ResolutionPolicyViolation[] {
  const seen = new Set<string>()
  const out: ResolutionPolicyViolation[] = []
  for (const violation of raw) {
    const key = `${violation.name}\0${violation.version}\0${violation.code}\0${violation.reason}`
    if (!seen.has(key)) {
      seen.add(key)
      out.push(violation)
    }
  }
  return out
}
