import path from 'node:path'

import type { Catalogs } from '@pnpm/catalogs.types'
import * as dp from '@pnpm/deps.path'
import { linkFileDepsInsidePackage } from '@pnpm/installing.package-requester'
import type { LockfileObject, PackageSnapshot } from '@pnpm/lockfile.types'
import { isNonLocalFilesystemSubdepSpec } from '@pnpm/resolving.local-resolver'
import type { Resolution, WorkspacePackages } from '@pnpm/resolving.resolver-base'
import type { StoreController } from '@pnpm/store.controller-types'
import type { DependencyManifest, DepPath, PackageExtension, PackageManifest, ReadPackageHook } from '@pnpm/types'
import normalize from 'normalize-path'
import { clone } from 'ramda'

import {
  appendPristineDeclaredSpecs,
  appendProbedHookEdges,
  computeWorkspaceDepth,
  createLocalResolutionPolicy,
  type LiveWorkspaceProject,
  type LocalResolutionPolicy,
  normalizeRelDir,
  policyHasChildRangedEdges,
} from './verifyLockfileLocalPolicy.js'
import { collectLocalResolutionViolations } from './verifyLockfileLocalResolutions.js'
import {
  collectAuthorizedLocalRoots,
  type IndexedSnapshot,
  indexSnapshotsByBase,
  isSnapshotBackedByLocalResolution,
} from './verifyLockfileLocalRoots.js'
import {
  loadPristineCandidateManifest,
  PROBE_FIELDS,
  type ProbeCandidate,
  recordPristineSpecs,
} from './verifyLockfilePristineManifests.js'
import { verifyLockfileResolutionShapes } from './verifyLockfileResolutions.js'
import {
  hasLocalFilesystemResolution,
  hasLocalGitResolution,
  isLocalGitPackage,
} from './verifyLockfileShape.js'

const PROBE_SENTINEL_SPEC = '0.0.0'
const PROBE_ALT_SENTINEL_SPEC = '0.0.0-pnpm-probe'

export interface InstallLocalPolicyOptions {
  allProjects?: LiveWorkspaceProject[]
  catalogs?: Catalogs
  deploy?: boolean
  hooks?: {
    readPackage?: ReadPackageHook[]
    afterAllResolved?: unknown[]
    untrackedPnpmfileReadPackageHook?: boolean
  }
  ignorePackageManifest?: boolean
  lockfileDir?: string
  overrides?: Record<string, string>
  overridesRootDir?: string
  packageExtensions?: Record<string, PackageExtension>
  readPackageHook?: ReadPackageHook
  selectedImporterIds?: Iterable<string>
  storeController?: StoreController
  workspaceDir?: string
  workspacePackages?: WorkspacePackages
}

export function localPolicyForInstallOptions (
  opts: InstallLocalPolicyOptions,
  liveProjects?: Iterable<LiveWorkspaceProject>
): LocalResolutionPolicy {
  const effectiveRoot = opts.workspaceDir ?? opts.lockfileDir
  const effectiveLiveProjects = opts.ignorePackageManifest ? undefined : (liveProjects ?? opts.allProjects)
  const effectiveWorkspacePackages = opts.ignorePackageManifest ? undefined : opts.workspacePackages
  return createLocalResolutionPolicy({
    catalogs: opts.catalogs,
    deploy: opts.deploy,
    extraWorkspaceDirs: collectExtraWorkspaceDirs(opts.lockfileDir, effectiveWorkspacePackages),
    hasAfterAllResolvedHook: Boolean(opts.hooks?.afterAllResolved?.length),
    hasPnpmfileHook: Boolean(
      opts.hooks?.readPackage?.length ||
      opts.hooks?.untrackedPnpmfileReadPackageHook
    ),
    liveWorkspaceDirs: collectLiveWorkspaceDirs(opts.lockfileDir, effectiveLiveProjects, effectiveWorkspacePackages),
    lockfileDir: opts.lockfileDir,
    overrides: opts.overrides,
    overridesRootDir: opts.overridesRootDir,
    packageExtensions: opts.packageExtensions,
    selectedImporterIds: normalizeSelectedImporterIds(opts.selectedImporterIds),
    workspaceDepthAboveLockfile: computeWorkspaceDepth(opts.lockfileDir, effectiveRoot),
    workspaceDir: effectiveRoot,
  })
}

export async function resolveLocalPolicyForInstall (
  lockfile: LockfileObject,
  opts: InstallLocalPolicyOptions,
  liveProjects?: Iterable<LiveWorkspaceProject>
): Promise<LocalResolutionPolicy> {
  const basePolicy = localPolicyForInstallOptions(opts, liveProjects)
  if (collectLocalResolutionViolations(lockfile, basePolicy).length === 0) {
    return basePolicy
  }
  verifyLockfileResolutionShapes(lockfile, basePolicy)
  const activeHooks = selectActiveReadPackageHooks(lockfile, opts.hooks?.readPackage)
  const needsRanged = policyHasChildRangedEdges(basePolicy)
  if (activeHooks == null && !needsRanged) return basePolicy
  const candidates = collectProbeCandidates(lockfile, basePolicy)
  if (candidates.length === 0) return basePolicy
  return enrichPolicyFromCandidates(basePolicy, candidates, { activeHooks, needsRanged, opts })
}

function selectActiveReadPackageHooks (
  lockfile: LockfileObject,
  hooks: ReadPackageHook[] | undefined
): ReadPackageHook[] | undefined {
  if (!hooks?.length) return undefined
  const lockfileHasHook = lockfile.pnpmfileChecksum != null || lockfile.untrackedPnpmfileReadPackageHook === true
  return lockfileHasHook ? hooks : undefined
}

async function enrichPolicyFromCandidates (
  basePolicy: LocalResolutionPolicy,
  candidates: ProbeCandidate[],
  ctx: {
    activeHooks: ReadPackageHook[] | undefined
    needsRanged: boolean
    opts: InstallLocalPolicyOptions
  }
): Promise<LocalResolutionPolicy> {
  const pristineSpecs = new Map<string, Record<string, string>>()
  const rewrites: Array<{ parentBaseKey: string, parentName: string, parentRange?: string, childAlias: string, spec: string }> = []
  for (const candidate of candidates) {
    // eslint-disable-next-line no-await-in-loop -- sequential probing avoids interleaving stateful user hooks
    await probeSingleCandidate(candidate, ctx, { pristineSpecs, rewrites })
  }
  return appendProbedHookEdges(appendPristineDeclaredSpecs(basePolicy, pristineSpecs), rewrites)
}

async function probeSingleCandidate (
  candidate: ProbeCandidate,
  { activeHooks, needsRanged, opts }: {
    activeHooks: ReadPackageHook[] | undefined
    needsRanged: boolean
    opts: InstallLocalPolicyOptions
  },
  acc: {
    pristineSpecs: Map<string, Record<string, string>>
    rewrites: Array<{ parentBaseKey: string, parentName: string, parentRange?: string, childAlias: string, spec: string }>
  }
): Promise<void> {
  const pristine = await loadPristineCandidateManifest(candidate, opts.storeController, opts.lockfileDir)
  if (needsRanged && pristine != null) recordPristineSpecs(candidate, pristine, acc.pristineSpecs)
  if (activeHooks == null) return
  // Fresh resolution rewrites in-archive `file:` dependencies before hooks
  // run, so the replay and its rewrite baseline do too.
  const prepared = pristine != null ? linkFileDepsInsidePackage(pristine as DependencyManifest) : undefined
  const hooked = await runReadPackageProbe(candidate, prepared, activeHooks, opts.lockfileDir, PROBE_SENTINEL_SPEC)
  const altHooked = prepared == null
    ? await runReadPackageProbe(candidate, undefined, activeHooks, opts.lockfileDir, PROBE_ALT_SENTINEL_SPEC)
    : undefined
  if (hooked != null) recordHookedCandidateRewrites(candidate, { pristine: prepared, hooked, altHooked }, acc.rewrites)
}

async function runReadPackageProbe (
  candidate: ProbeCandidate,
  pristine: PackageManifest | undefined,
  hooks: ReadPackageHook[],
  lockfileDir: string | undefined,
  sentinel: string
): Promise<PackageManifest | undefined> {
  let pkg: PackageManifest = pristine != null
    ? clone(pristine)
    : {
      name: candidate.name,
      version: candidate.version ?? sentinel,
      dependencies: sentinelMap(candidate.dependencies, sentinel),
      optionalDependencies: sentinelMap(candidate.optionalDependencies, sentinel),
      peerDependencies: sentinelMap(candidate.peerDependencies, sentinel),
      devDependencies: {},
    }
  try {
    for (const hook of hooks) {
      // eslint-disable-next-line no-await-in-loop -- hooks compose sequentially on the manifest
      pkg = (await hook(pkg, lockfileDir)) ?? pkg
    }
    return pkg
  } catch (err: unknown) {
    // The hook may not handle a placeholder manifest. Only an error on the
    // real manifest is one the install would also hit.
    if (pristine != null) throw err
    return undefined
  }
}

function sentinelMap (aliases: Set<string>, sentinel: string): Record<string, string> {
  const map: Record<string, string> = {}
  for (const alias of aliases) map[alias] = sentinel
  return map
}

function recordHookedCandidateRewrites (
  candidate: ProbeCandidate,
  { pristine, hooked, altHooked }: {
    pristine: PackageManifest | undefined
    hooked: PackageManifest
    altHooked: PackageManifest | undefined
  },
  out: Array<{ parentBaseKey: string, parentName: string, parentRange?: string, childAlias: string, spec: string }>
): void {
  for (const field of PROBE_FIELDS) {
    recordFieldRewrites(candidate, { preMap: pristine?.[field], postMap: hooked[field], altMap: altHooked?.[field], hasPristine: pristine != null }, out)
  }
}

function recordFieldRewrites (
  candidate: ProbeCandidate,
  { preMap, postMap, altMap, hasPristine }: {
    preMap: Record<string, string> | undefined
    postMap: Record<string, string> | undefined
    altMap: Record<string, string> | undefined
    hasPristine: boolean
  },
  out: Array<{ parentBaseKey: string, parentName: string, parentRange?: string, childAlias: string, spec: string }>
): void {
  for (const [alias, value] of Object.entries(postMap ?? {})) {
    if (typeof value !== 'string') continue
    const rewritten = hasPristine
      ? value !== preMap?.[alias]
      : value !== PROBE_SENTINEL_SPEC && value !== PROBE_ALT_SENTINEL_SPEC && altMap?.[alias] === value
    if (rewritten) {
      out.push({
        parentBaseKey: candidate.baseKey,
        parentName: candidate.name,
        parentRange: candidate.version,
        childAlias: alias,
        spec: value,
      })
    }
  }
}

function collectProbeCandidates (lockfile: LockfileObject, policy: LocalResolutionPolicy): ProbeCandidate[] {
  const byBaseKey = new Map<string, ProbeCandidate>()
  const snapshotsByBase = indexSnapshotsByBase(lockfile)
  const reachableFilter = collectReachableSnapshotsFilter(lockfile, policy)
  for (const [depPath, snapshot] of Object.entries(lockfile.packages ?? {}) as Array<[DepPath, PackageSnapshot]>) {
    if (reachableFilter != null && !reachableFilter.has(depPath)) continue
    const parsed = dp.parse(depPath)
    if (!isEligibleProbeSnapshot(parsed.nonSemverVersion, snapshot)) continue
    addSnapshotProbeAliases(byBaseKey, { lockfile, snapshotsByBase, depPath, parsed, snapshot })
  }
  return Array.from(byBaseKey.values())
}

function isEligibleProbeSnapshot (nonSemverVersion: string | undefined, snapshot: PackageSnapshot): boolean {
  if (isSnapshotBackedByLocalResolution(nonSemverVersion, snapshot) || isLocalGitPackage(nonSemverVersion, snapshot.resolution)) {
    return false
  }
  return !hasLocalFilesystemResolution(snapshot.resolution) && !hasLocalGitResolution(snapshot.resolution)
}

function collectReachableSnapshotsFilter (
  lockfile: LockfileObject,
  policy: LocalResolutionPolicy
): Set<DepPath> | undefined {
  if (policy.selectedImporterIds == null) return undefined
  const roots = collectAuthorizedLocalRoots(lockfile, policy)
  const snaps = new Set<DepPath>()
  for (const imp of roots.importers) {
    for (const depPath of imp.reachableSnapshots) snaps.add(depPath)
  }
  return snaps
}

function addSnapshotProbeAliases (
  byBaseKey: Map<string, ProbeCandidate>,
  { lockfile, snapshotsByBase, depPath, parsed, snapshot }: {
    lockfile: LockfileObject
    snapshotsByBase: Map<string, IndexedSnapshot[]>
    depPath: DepPath
    parsed: ReturnType<typeof dp.parse>
    snapshot: PackageSnapshot
  }
): void {
  const deps = findLocalChildAliases(lockfile, snapshotsByBase, snapshot.dependencies)
  const opts = findLocalChildAliases(lockfile, snapshotsByBase, snapshot.optionalDependencies)
  const peers = findLocalPeerAliases(snapshot.peerDependencies)
  if (deps.length === 0 && opts.length === 0 && peers.length === 0) return
  const baseKey = dp.removeSuffix(depPath)
  let entry = byBaseKey.get(baseKey)
  if (entry == null) {
    entry = {
      baseKey,
      name: parsed.name ?? snapshot.name ?? depPath,
      version: parsed.version ?? snapshot.version,
      nonSemverVersion: parsed.nonSemverVersion,
      resolution: snapshot.resolution as Resolution,
      dependencies: new Set(),
      optionalDependencies: new Set(),
      peerDependencies: new Set(),
    }
    byBaseKey.set(baseKey, entry)
  }
  for (const alias of deps) entry.dependencies.add(alias)
  for (const alias of opts) entry.optionalDependencies.add(alias)
  for (const alias of peers) entry.peerDependencies.add(alias)
}

function findLocalChildAliases (
  lockfile: LockfileObject,
  snapshotsByBase: Map<string, IndexedSnapshot[]>,
  deps: Record<string, string> | undefined
): string[] {
  if (deps == null) return []
  const out: string[] = []
  for (const [alias, ref] of Object.entries(deps)) {
    if (isLocalChildRef(lockfile, snapshotsByBase, alias, ref)) out.push(alias)
  }
  return out
}

function isLocalChildRef (
  lockfile: LockfileObject,
  snapshotsByBase: Map<string, IndexedSnapshot[]>,
  alias: string,
  ref: string
): boolean {
  if (ref.startsWith('link:')) return dp.packageRootLinkTarget(ref) == null
  const childDepPath = dp.refToRelative(ref, alias)
  if (childDepPath == null) return false
  const nonSemver = dp.parse(childDepPath).nonSemverVersion
  if (nonSemver?.startsWith('file:')) return true
  const baseKey = dp.removeSuffix(childDepPath)
  const snap = lockfile.packages?.[childDepPath] ?? lockfile.packages?.[baseKey as DepPath] ?? snapshotsByBase.get(baseKey)?.[0]?.snapshot
  return isLocalGitPackage(nonSemver, snap?.resolution) || (snap != null && hasLocalFilesystemResolution(snap.resolution))
}

function findLocalPeerAliases (peers: Record<string, string> | undefined): string[] {
  if (peers == null) return []
  return Object.entries(peers)
    .filter(([, spec]) => dp.packageRootLinkTarget(spec) == null && isNonLocalFilesystemSubdepSpec(spec, () => false))
    .map(([alias]) => alias)
}

function normalizeSelectedImporterIds (ids: Iterable<string> | undefined): Set<string> | undefined {
  if (ids == null) return undefined
  const out = new Set<string>()
  for (const id of ids) out.add(normalizeRelDir(id))
  return out
}

function collectExtraWorkspaceDirs (
  lockfileDir: string | undefined,
  workspacePackages: WorkspacePackages | undefined
): Set<string> | undefined {
  if (!lockfileDir || !workspacePackages) return undefined
  const dirs = new Set<string>()
  for (const versions of workspacePackages.values()) {
    for (const pkg of versions.values()) {
      dirs.add(normalizeRelDir(normalize(path.relative(lockfileDir, pkg.rootDir))))
    }
  }
  return dirs
}

function collectLiveWorkspaceDirs (
  lockfileDir: string | undefined,
  liveProjects: Iterable<LiveWorkspaceProject> | undefined,
  workspacePackages: WorkspacePackages | undefined
): Set<string> | undefined {
  if (!lockfileDir || (liveProjects == null && workspacePackages == null)) return undefined
  const dirs = new Set<string>()
  for (const project of liveProjects ?? []) addLiveProjectDirs(lockfileDir, project, dirs)
  for (const versions of workspacePackages?.values() ?? []) {
    for (const pkg of versions.values()) addLiveProjectDirs(lockfileDir, pkg, dirs)
  }
  return dirs.size > 0 ? dirs : undefined
}

function addLiveProjectDirs (lockfileDir: string, project: LiveWorkspaceProject, out: Set<string>): void {
  out.add(normalizeRelDir(normalize(path.relative(lockfileDir, project.rootDir))))
  const pubDir = project.manifest?.publishConfig?.directory
  if (pubDir) {
    out.add(normalizeRelDir(normalize(path.relative(lockfileDir, path.resolve(project.rootDir, pubDir)))))
  }
}
