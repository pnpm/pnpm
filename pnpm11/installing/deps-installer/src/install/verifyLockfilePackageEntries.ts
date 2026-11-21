import * as dp from '@pnpm/deps.path'
import type { LockfileObject, PackageSnapshot } from '@pnpm/lockfile.types'
import { isNonLocalFilesystemSubdepSpec } from '@pnpm/resolving.local-resolver'
import type { Resolution, ResolutionPolicyViolation } from '@pnpm/resolving.resolver-base'
import type { DepPath } from '@pnpm/types'

import {
  hasInnerParentSegment,
  isNonRelativeTarget,
  type LocalResolutionPolicy,
  normalizeRelDir,
  policyAuthorizesPeerSpec,
  stripFileUrlPrefix,
  targetEscapesWorkspace,
} from './verifyLockfileLocalPolicy.js'
import { isSnapshotBackedByLocalResolution } from './verifyLockfileLocalRoots.js'
import {
  extractLocalGitTarget,
  fileKeyMismatchesLocalTarget,
  hasLocalFilesystemResolution,
  isLocalGitPackage,
  localGitKeyMismatchesTarget,
  localGitKeyRawPath,
} from './verifyLockfileShape.js'

const LOCAL_SUBDEP_CODE = 'LOCAL_SUBDEP_NOT_ALLOWED'
const SHAPE_MISMATCH_CODE = 'RESOLUTION_SHAPE_MISMATCH'

export interface PackageEntryScan {
  lockfile: LockfileObject
  policy: LocalResolutionPolicy
  workspaceDirs: Set<string>
  authorizedPackages: Set<string>
  authorizedGitPackages: Set<string>
  reachableFilter?: Set<string>
  /** Snapshots the selected importers reach. Peers resolved only elsewhere do not count. */
  selectedSnapshots?: Set<DepPath>
  /** Built by `authorizedLocalPackageDeclaresPeer` on first use. */
  authorizedPeerDeclarations?: Set<string>
}

export function collectPackageEntryViolations (scan: PackageEntryScan, violations: ResolutionPolicyViolation[]): void {
  const shapeCtx = {
    allowDeployFileUrlKeys: Boolean(scan.policy.allowDeployFileUrlKeys),
    lockfileDir: scan.policy.lockfileDir,
  }
  for (const [depPath, snapshot] of Object.entries(scan.lockfile.packages ?? {}) as Array<[DepPath, PackageSnapshot]>) {
    const baseKey = dp.removeSuffix(depPath)
    if (scan.reachableFilter != null && !scan.reachableFilter.has(baseKey)) continue
    inspectPackageEntry(scan, shapeCtx, { depPath, baseKey, snapshot }, violations)
  }
}

function inspectPackageEntry (
  scan: PackageEntryScan,
  shapeCtx: { allowDeployFileUrlKeys: boolean, lockfileDir: string | undefined },
  { depPath, baseKey, snapshot }: { depPath: DepPath, baseKey: string, snapshot: PackageSnapshot },
  violations: ResolutionPolicyViolation[]
): void {
  const parsed = dp.parse(depPath)
  const pkgName = parsed.name ?? snapshot.name ?? depPath
  const pkgVer = parsed.version ?? snapshot.version
  const displayVer = pkgVer ?? parsed.nonSemverVersion ?? '0.0.0'
  if (scan.selectedSnapshots == null || scan.selectedSnapshots.has(depPath)) {
    checkSnapshotPeerSpecs(scan, { name: pkgName, version: pkgVer, displayVer, baseKey, nonSemverVersion: parsed.nonSemverVersion, peerSuffix: parsed.peerDepGraphHash, snapshot }, violations)
  }
  if (parsed.nonSemverVersion?.startsWith('file:')) {
    checkFilePackageEntry(scan, shapeCtx, { pkgName, baseKey, nonSemverVersion: parsed.nonSemverVersion, snapshot }, violations)
    return
  }
  const gitKeyPath = localGitKeyRawPath(parsed.nonSemverVersion)
  if (gitKeyPath != null && parsed.nonSemverVersion != null) {
    checkLocalGitPackageEntry(scan, shapeCtx, { pkgName, baseKey, gitKeyPath, nonSemverVersion: parsed.nonSemverVersion, snapshot }, violations)
  }
}

function checkFilePackageEntry (
  scan: PackageEntryScan,
  shapeCtx: { allowDeployFileUrlKeys: boolean, lockfileDir: string | undefined },
  { pkgName, baseKey, nonSemverVersion, snapshot }: { pkgName: string, baseKey: string, nonSemverVersion: string, snapshot: PackageSnapshot },
  violations: ResolutionPolicyViolation[]
): void {
  if (fileKeyMismatchesLocalTarget(nonSemverVersion, snapshot.resolution, shapeCtx)) {
    violations.push(packageViolation(pkgName, nonSemverVersion, snapshot, SHAPE_MISMATCH_CODE, 'a file dependency path does not match its local resolution target'))
    return
  }
  const isAuthorized = isDeploySnapshot(scan.policy, nonSemverVersion, snapshot) ||
    scan.authorizedPackages.has(baseKey) ||
    isWorkspaceImporterDirectory(scan.policy, scan.workspaceDirs, nonSemverVersion, snapshot)
  if (!isAuthorized && resolutionEscapesWorkspace(snapshot.resolution, scan.policy)) {
    violations.push(packageViolation(pkgName, nonSemverVersion, snapshot, LOCAL_SUBDEP_CODE, 'a local filesystem package entry escapes outside the workspace without authorization'))
  }
}

function checkLocalGitPackageEntry (
  scan: PackageEntryScan,
  shapeCtx: { allowDeployFileUrlKeys: boolean, lockfileDir: string | undefined },
  { pkgName, baseKey, gitKeyPath, nonSemverVersion, snapshot }: {
    pkgName: string
    baseKey: string
    gitKeyPath: string
    nonSemverVersion: string
    snapshot: PackageSnapshot
  },
  violations: ResolutionPolicyViolation[]
): void {
  if (localGitKeyMismatchesTarget(gitKeyPath, snapshot.resolution, shapeCtx)) {
    violations.push(packageViolation(pkgName, nonSemverVersion, snapshot, SHAPE_MISMATCH_CODE, 'a local git dependency path does not match its git repository target'))
  } else if (!scan.authorizedGitPackages.has(baseKey) && resolutionEscapesWorkspace(snapshot.resolution, scan.policy)) {
    violations.push(packageViolation(pkgName, nonSemverVersion, snapshot, LOCAL_SUBDEP_CODE, 'a local git package entry escapes outside the workspace without authorization'))
  }
}

function checkSnapshotPeerSpecs (
  scan: PackageEntryScan,
  pkg: { name: string, version?: string, displayVer: string, baseKey: string, nonSemverVersion?: string, peerSuffix?: string, snapshot: PackageSnapshot },
  violations: ResolutionPolicyViolation[]
): void {
  const peers = pkg.snapshot.peerDependencies
  if (peers == null || snapshotSkipsPeerCheck(scan, pkg)) return
  const isAuthorizedGit = isLocalGitPackage(pkg.nonSemverVersion, pkg.snapshot.resolution) &&
    scan.authorizedGitPackages.has(pkg.baseKey)
  for (const [alias, spec] of Object.entries(peers)) {
    if (isPeerResolved(pkg, alias) && isUnauthorizedPeerSpec(scan, pkg, { alias, spec, isAuthorizedGit })) {
      violations.push(packageViolation(pkg.name, pkg.displayVer, pkg.snapshot, LOCAL_SUBDEP_CODE, 'a package entry declares an unauthorized local filesystem peer dependency'))
    }
  }
}

/**
 * A peer left unresolved (for example under `autoInstallPeers: false`)
 * creates no edge, so its declaration reaches nothing on disk.
 */
function isPeerResolved (
  { peerSuffix, snapshot }: { peerSuffix?: string, snapshot: PackageSnapshot },
  alias: string
): boolean {
  const takesAlias = snapshot.dependencies?.[alias] != null || snapshot.optionalDependencies?.[alias] != null
  return takesAlias && peerSuffixBinds(peerSuffix, alias)
}

/**
 * Whether the dependency path's peer suffix binds `alias`. A regular
 * dependency of the same name is not a resolved peer. A hashed group names
 * no peers, so it counts as binding every alias.
 */
function peerSuffixBinds (peerSuffix: string | undefined, alias: string): boolean {
  if (!peerSuffix) return false
  return peerSuffix.includes(`(${alias}@`) || topLevelGroups(peerSuffix).some(isHashGroup)
}

function topLevelGroups (suffix: string): string[] {
  const groups: string[] = []
  let depth = 0
  let start = 0
  for (let index = 0; index < suffix.length; index++) {
    const delta = parenDepthDelta(suffix[index])
    if (delta === 1 && depth === 0) start = index + 1
    depth += delta
    if (delta === -1 && depth === 0) groups.push(suffix.slice(start, index))
  }
  return groups
}

function parenDepthDelta (char: string): number {
  if (char === '(') return 1
  return char === ')' ? -1 : 0
}

/**
 * A peer graph hash group holds only the hash. Explicit peer groups hold
 * `name@version` and the patch group holds `patch_hash=...`.
 */
function isHashGroup (group: string): boolean {
  return group !== '' && Array.from(group).every(isAsciiAlphanumeric)
}

function isAsciiAlphanumeric (char: string): boolean {
  return (char >= '0' && char <= '9') || (char >= 'a' && char <= 'z') || (char >= 'A' && char <= 'Z')
}

function snapshotSkipsPeerCheck (
  scan: PackageEntryScan,
  pkg: { baseKey: string, nonSemverVersion?: string, snapshot: PackageSnapshot }
): boolean {
  if (!isSnapshotBackedByLocalResolution(pkg.nonSemverVersion, pkg.snapshot)) return false
  return isDeploySnapshot(scan.policy, pkg.nonSemverVersion, pkg.snapshot) ||
    scan.authorizedPackages.has(pkg.baseKey) ||
    isWorkspaceImporterDirectory(scan.policy, scan.workspaceDirs, pkg.nonSemverVersion, pkg.snapshot)
}

function isUnauthorizedPeerSpec (
  scan: PackageEntryScan,
  pkg: { baseKey: string, name: string, version?: string },
  { alias, spec, isAuthorizedGit }: { alias: string, spec: string, isAuthorizedGit: boolean }
): boolean {
  if (isAuthorizedGit && /^git\+file:/i.test(spec.trim())) return false
  if (dp.packageRootLinkTarget(spec) != null) return false
  return isNonLocalFilesystemSubdepSpec(spec, () => false) &&
    !policyAuthorizesPeerSpec(scan.policy, scan.lockfile, pkg, alias, spec) &&
    !authorizedLocalPackageDeclaresPeer(scan, alias, spec)
}

/**
 * Whether an authorized local package declares the same peer, which lets
 * resolution hoist that local peer for a non-local package too.
 */
function authorizedLocalPackageDeclaresPeer (scan: PackageEntryScan, alias: string, spec: string): boolean {
  scan.authorizedPeerDeclarations ??= indexAuthorizedPeerDeclarations(scan)
  return scan.authorizedPeerDeclarations.has(peerDeclarationKey(alias, spec))
}

function indexAuthorizedPeerDeclarations (scan: PackageEntryScan): Set<string> {
  const declarations = new Set<string>()
  for (const [depPath, snapshot] of Object.entries(scan.lockfile.packages ?? {})) {
    if (snapshot.peerDependencies == null || !scan.authorizedPackages.has(dp.removeSuffix(depPath as DepPath))) continue
    for (const [alias, spec] of Object.entries(snapshot.peerDependencies) as Array<[string, string]>) {
      declarations.add(peerDeclarationKey(alias, spec))
    }
  }
  return declarations
}

function peerDeclarationKey (alias: string, spec: string): string {
  return `${alias}\0${spec}`
}

function resolutionEscapesWorkspace (resolution: unknown, policy: LocalResolutionPolicy): boolean {
  if (resolution == null || typeof resolution !== 'object') return false
  const { type, directory, tarball, repo, variants } = resolution as {
    type?: unknown
    directory?: unknown
    tarball?: unknown
    repo?: unknown
    variants?: unknown
  }
  if (type === 'directory' && typeof directory === 'string') return targetEscapesWorkspace(policy, directory)
  if (type === 'variations' && Array.isArray(variants)) {
    return variants.some((variant) => resolutionEscapesWorkspace((variant as { resolution?: unknown })?.resolution, policy))
  }
  return gitOrTarballEscapes({ type, tarball, repo }, policy)
}

function gitOrTarballEscapes (
  { type, tarball, repo }: { type?: unknown, tarball?: unknown, repo?: unknown },
  policy: LocalResolutionPolicy
): boolean {
  if (type === 'git' && typeof repo === 'string') {
    const gitTarget = extractLocalGitTarget(repo)
    return gitTarget != null && targetEscapesWorkspace(policy, gitTarget)
  }
  const tarPath = typeof tarball === 'string' ? stripFileUrlPrefix(tarball) : undefined
  return tarPath != null && targetEscapesWorkspace(policy, tarPath)
}

export function isAllowedWorkspaceTarget (workspaceDirs: Set<string>, target: string): boolean {
  return !isNonRelativeTarget(target) && !hasInnerParentSegment(target) && workspaceDirs.has(normalizeRelDir(target))
}

export function isDeploySnapshot (
  policy: LocalResolutionPolicy,
  nonSemverVersion: string | undefined,
  snapshot: PackageSnapshot | undefined
): boolean {
  if (!policy.allowDeployFileUrlKeys || !nonSemverVersion?.startsWith('file://') || snapshot == null) return false
  return hasLocalFilesystemResolution(snapshot.resolution) &&
    !fileKeyMismatchesLocalTarget(nonSemverVersion, snapshot.resolution, {
      allowDeployFileUrlKeys: true,
      lockfileDir: policy.lockfileDir,
    })
}

export function isWorkspaceImporterDirectory (
  policy: LocalResolutionPolicy,
  workspaceDirs: Set<string>,
  nonSemverVersion: string | undefined,
  snapshot: PackageSnapshot | undefined
): boolean {
  const resolution = snapshot?.resolution as { type?: unknown, directory?: unknown } | undefined
  if (resolution?.type === 'directory' && typeof resolution.directory === 'string') {
    return isAllowedWorkspaceTarget(workspaceDirs, resolution.directory)
  }
  if (resolution == null && policy.allowDeployFileUrlKeys && nonSemverVersion?.startsWith('file:')) {
    return isAllowedWorkspaceTarget(workspaceDirs, nonSemverVersion.slice(5))
  }
  return false
}

export function packageViolation (
  name: string,
  version: string,
  snapshot: PackageSnapshot | undefined,
  code: string,
  reason: string
): ResolutionPolicyViolation {
  const resolution = (snapshot?.resolution as Resolution | undefined) ?? { type: 'directory', directory: version }
  return { name, version, resolution, code, reason }
}
