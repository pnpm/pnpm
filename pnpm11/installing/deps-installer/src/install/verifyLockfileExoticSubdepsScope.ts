import crypto from 'node:crypto'
import path from 'node:path'

import * as dp from '@pnpm/deps.path'
import type { PnpmContext } from '@pnpm/installing.context'
import {
  getLockfileImporterId,
  type LockfileObject,
  type PackageSnapshot,
  type PackageSnapshots,
  type ProjectSnapshot,
  type ResolvedDependencies,
} from '@pnpm/lockfile.fs'
import { isFilespec, localSpecPath } from '@pnpm/resolving.local-resolver'
import type { WorkspacePackages } from '@pnpm/resolving.resolver-base'
import type { DepPath, ProjectManifest } from '@pnpm/types'

import {
  isCanonicalImporterId,
  isOwnPublishDir,
  isRealWorkspaceTarget,
  joinPosixRelPath,
  normalizePosixRelPath,
  readManifestIfExists,
} from './verifyLockfileExoticSubdepsPaths.js'
import {
  canTrustPolicySnapshot,
  mergePolicies,
  type VerifierCacheIdentity,
} from './verifyLockfileResolutionsCache.js'

export { isCanonicalImporterId, joinPosixRelPath, normalizePosixRelPath }

export interface ExoticSubdepsScope {
  knownImporterIds?: ReadonlySet<string>
  extraWorkspaceDirs?: readonly string[]
  nodeMirrors?: readonly string[]
}

export interface NormalizedExoticSubdepsScope {
  knownImporterIds?: Set<string>
  extraWorkspaceDirs: string[]
  nodeMirrors: string[]
}

export interface ExoticSubdepsScopeOptions {
  blockExoticSubdeps?: boolean
  trustLockfile?: boolean
  ignorePackageManifest?: boolean
  workspacePackages?: WorkspacePackages
  deployedWorkspaceDirs?: readonly string[]
  nodeDownloadMirrors?: Record<string, string>
}

interface WorkspaceTraversalState {
  lockfileDir: string
  manifestCache: Map<string, ProjectManifest | null>
  visitedTargets: Set<string>
}

export function buildExoticSubdepsScopeFromContext (
  ctx: Pick<PnpmContext, 'lockfileDir' | 'projects'> & Partial<Pick<PnpmContext, 'workspacePackages'>>,
  opts: ExoticSubdepsScopeOptions
): ExoticSubdepsScope {
  if (opts.trustLockfile || opts.blockExoticSubdeps === false) return {}
  return buildScopeFromEntries(
    ctx.lockfileDir,
    Object.values(ctx.projects).map((project) => ({
      id: project.id,
      rootDir: project.rootDir,
      manifest: project.manifest,
    })),
    { ...opts, workspacePackages: opts.workspacePackages ?? ctx.workspacePackages }
  )
}

export function buildExoticSubdepsScopeFromProjectDirs (
  lockfileDir: string,
  projectDirs: readonly string[],
  opts: ExoticSubdepsScopeOptions
): ExoticSubdepsScope {
  if (opts.trustLockfile || opts.blockExoticSubdeps === false) return {}
  return buildScopeFromEntries(
    lockfileDir,
    projectDirs.map((rootDir) => ({
      id: getLockfileImporterId(lockfileDir, rootDir),
      rootDir,
      manifest: readManifestIfExists(rootDir),
    })),
    opts
  )
}

function buildScopeFromEntries (
  lockfileDir: string,
  entries: Array<{ id: string, rootDir: string, manifest: ProjectManifest | undefined }>,
  opts: ExoticSubdepsScopeOptions
): ExoticSubdepsScope {
  const knownImporterIds = new Set<string>(entries.map((entry) => entry.id))
  addWorkspacePackageImporterIds(knownImporterIds, lockfileDir, opts.workspacePackages)
  const extraWorkspaceDirs = new Set<string>(opts.deployedWorkspaceDirs ?? [])
  const traversal: WorkspaceTraversalState = {
    lockfileDir,
    manifestCache: new Map(),
    visitedTargets: new Set(),
  }
  for (const entry of entries) {
    addVerifiedManifestDirs(extraWorkspaceDirs, { rootDir: entry.rootDir, manifest: entry.manifest }, traversal)
  }
  addVerifiedWorkspacePackageDirs(extraWorkspaceDirs, opts.workspacePackages, traversal)
  return {
    knownImporterIds,
    extraWorkspaceDirs: Array.from(extraWorkspaceDirs),
    nodeMirrors: opts.nodeDownloadMirrors != null ? Object.values(opts.nodeDownloadMirrors) : undefined,
  }
}

function addWorkspacePackageImporterIds (
  knownImporterIds: Set<string>,
  lockfileDir: string,
  workspacePackages: WorkspacePackages | undefined
): void {
  if (workspacePackages == null) return
  for (const versions of workspacePackages.values()) {
    for (const pkg of versions.values()) {
      knownImporterIds.add(getLockfileImporterId(lockfileDir, pkg.rootDir))
    }
  }
}

function addVerifiedWorkspacePackageDirs (
  dirs: Set<string>,
  workspacePackages: WorkspacePackages | undefined,
  traversal: WorkspaceTraversalState
): void {
  if (workspacePackages == null) return
  for (const versions of workspacePackages.values()) {
    for (const pkg of versions.values()) {
      addVerifiedManifestDirs(dirs, { rootDir: pkg.rootDir, manifest: pkg.manifest }, traversal)
    }
  }
}

function addVerifiedManifestDirs (
  dirs: Set<string>,
  location: { rootDir: string, manifest: ProjectManifest | undefined },
  traversal: WorkspaceTraversalState
): void {
  const normalizedRoot = path.resolve(location.rootDir)
  recordWorkspaceRootAndPublishDir(dirs, traversal.lockfileDir, normalizedRoot, location.manifest)
  const queue: Array<{ dir: string, manifest: ProjectManifest }> = []
  collectDirectWorkspaceTargets(dirs, queue, {
    parentRoot: normalizedRoot,
    specs: manifestDependencySpecifiers(location.manifest, { includeDev: true }),
    traversal,
  })
  while (queue.length > 0) {
    const current = queue.shift()!
    collectDirectWorkspaceTargets(dirs, queue, {
      parentRoot: current.dir,
      specs: manifestDependencySpecifiers(current.manifest, { includeDev: false }),
      traversal,
    })
  }
}

function collectDirectWorkspaceTargets (
  dirs: Set<string>,
  queue: Array<{ dir: string, manifest: ProjectManifest }>,
  {
    parentRoot,
    specs,
    traversal,
  }: {
    parentRoot: string
    specs: IterableIterator<string>
    traversal: WorkspaceTraversalState
  }
): void {
  for (const rawSpec of specs) {
    const resolved = resolveWorkspacePathSpec(parentRoot, rawSpec, traversal.manifestCache)
    if (resolved == null || traversal.visitedTargets.has(resolved.dir)) continue
    traversal.visitedTargets.add(resolved.dir)
    recordWorkspaceRootAndPublishDir(dirs, traversal.lockfileDir, resolved.dir, resolved.manifest)
    queue.push(resolved)
  }
}

function recordWorkspaceRootAndPublishDir (
  dirs: Set<string>,
  lockfileDir: string,
  rootDir: string,
  manifest: ProjectManifest | undefined
): void {
  dirs.add(getLockfileImporterId(lockfileDir, rootDir))
  const publishConfig = manifest?.publishConfig
  if (publishConfig?.linkDirectory === false || typeof publishConfig?.directory !== 'string') return
  const publishDir = path.resolve(rootDir, publishConfig.directory)
  if (isOwnPublishDir(rootDir, publishDir)) {
    dirs.add(getLockfileImporterId(lockfileDir, publishDir))
  }
}

function * manifestDependencySpecifiers (
  manifest: ProjectManifest | undefined,
  { includeDev }: { includeDev: boolean }
): IterableIterator<string> {
  if (manifest == null) return
  const groups = includeDev
    ? [manifest.dependencies, manifest.devDependencies, manifest.optionalDependencies]
    : [manifest.dependencies, manifest.optionalDependencies]
  for (const deps of groups) {
    if (deps == null) continue
    for (const spec of Object.values(deps)) {
      if (typeof spec === 'string') yield spec
    }
  }
}

function resolveWorkspacePathSpec (
  projectRoot: string,
  rawSpec: string,
  manifestCache: Map<string, ProjectManifest | null>
): { dir: string, manifest: ProjectManifest } | undefined {
  const trimmed = rawSpec.trim()
  if (!trimmed.startsWith('workspace:') || !isFilespec(trimmed.slice('workspace:'.length))) return undefined
  const candidate = localSpecPath(trimmed, projectRoot)
  if (!isRealWorkspaceTarget(projectRoot, candidate)) return undefined
  let manifest = manifestCache.get(candidate)
  if (manifest === undefined) {
    manifest = readManifestIfExists(candidate) ?? null
    manifestCache.set(candidate, manifest)
  }
  return manifest != null ? { dir: candidate, manifest } : undefined
}

export function computeExoticSubdepsScopeDigest (scope?: ExoticSubdepsScope): string {
  const normalized = normalizeExoticSubdepsScope(scope)
  const hash = crypto.createHash('sha256')
  if (normalized.knownImporterIds == null) {
    hash.update('importers:*\0')
  } else {
    updateSortedSet(hash, 'importers:scoped\0', normalized.knownImporterIds)
  }
  updateSortedSet(hash, 'extra_dirs\0', normalized.extraWorkspaceDirs)
  updateSortedSet(hash, 'node_mirrors\0', normalized.nodeMirrors)
  return hash.digest('hex')
}

export function exoticSubdepsCacheIdentity (scope?: ExoticSubdepsScope): VerifierCacheIdentity {
  const scopeDigest = computeExoticSubdepsScopeDigest(scope)
  return {
    policy: { blockExoticSubdeps: true, blockExoticSubdepsScope: scopeDigest },
    canTrustPastCheck: (cached) => cached.blockExoticSubdeps === true && cached.blockExoticSubdepsScope === scopeDigest,
  }
}

export function snapshotPolicyIdentities (identities: readonly VerifierCacheIdentity[]): Record<string, unknown> {
  return mergePolicies(identities)
}

export function canTrustPolicyIdentities (
  recordedPolicy: Record<string, unknown>,
  identities: readonly VerifierCacheIdentity[]
): boolean {
  return canTrustPolicySnapshot(recordedPolicy, identities)
}

function updateU64BE (hash: crypto.Hash, value: number): void {
  const buf = Buffer.alloc(8)
  buf.writeBigUInt64BE(BigInt(value))
  hash.update(buf)
}

function updateSortedSet (hash: crypto.Hash, tag: string, items: Iterable<string>): void {
  const unique = Array.from(new Set(items)).sort()
  hash.update(tag)
  updateU64BE(hash, unique.length)
  for (const item of unique) {
    const bytes = Buffer.from(item, 'utf8')
    updateU64BE(hash, bytes.byteLength)
    hash.update(bytes)
  }
}

export function normalizeExoticSubdepsScope (scope?: ExoticSubdepsScope): NormalizedExoticSubdepsScope {
  return {
    knownImporterIds: scope?.knownImporterIds != null
      ? new Set(Array.from(scope.knownImporterIds, normalizePosixRelPath))
      : undefined,
    extraWorkspaceDirs: (scope?.extraWorkspaceDirs ?? []).map(normalizePosixRelPath),
    nodeMirrors: (scope?.nodeMirrors ?? [])
      .map(normalizeMirrorPrefix)
      .filter((prefix): prefix is string => prefix != null),
  }
}

function normalizeMirrorPrefix (raw: string): string | undefined {
  const trimmed = raw.trim()
  if (trimmed === '' || !/^https?:\/\//i.test(trimmed)) return undefined
  return trimmed.endsWith('/') ? trimmed : `${trimmed}/`
}

export function ownProp<Value> (record: Record<string, Value> | undefined, key: string): Value | undefined {
  return (record != null && Object.hasOwn(record, key)) ? record[key] : undefined
}

export function ownStringProp (record: Record<string, unknown> | undefined, key: string): string | undefined {
  if (record == null || !Object.hasOwn(record, key)) return undefined
  const value = record[key]
  return typeof value === 'string' ? value : undefined
}

export function lookupPackageSnapshot (
  packages: PackageSnapshots | undefined,
  depPath: DepPath
): PackageSnapshot | undefined {
  if (!packages) return undefined
  const table = packages as Record<string, PackageSnapshot | undefined>
  return ownProp(table, depPath) ?? ownProp(table, dp.removeSuffix(depPath))
}

function trustedImporterEntries (
  lockfile: LockfileObject,
  scope: NormalizedExoticSubdepsScope
): Array<[string, ProjectSnapshot]> {
  return Object.entries(lockfile.importers ?? {}).filter(([id]) =>
    isCanonicalImporterId(id) && (scope.knownImporterIds == null || scope.knownImporterIds.has(id))
  )
}

export function collectWorkspaceDirectories (lockfile: LockfileObject, scope: NormalizedExoticSubdepsScope): Set<string> {
  const dirs = new Set<string>(scope.extraWorkspaceDirs)
  for (const [importerId] of trustedImporterEntries(lockfile, scope)) {
    dirs.add(importerId)
  }
  return dirs
}

function extractDirectoryFromSnapshot (snapshot: PackageSnapshot | undefined): string | undefined {
  const resolution = snapshot?.resolution as { type?: unknown, directory?: unknown } | undefined
  if (resolution?.type === 'directory' && typeof resolution.directory === 'string') {
    return resolution.directory
  }
  return undefined
}

export function isWorkspaceSnapshot (snapshot: PackageSnapshot | undefined, workspaceDirs: Set<string>): boolean {
  const directory = extractDirectoryFromSnapshot(snapshot)
  return directory != null && !directory.includes('\\') && workspaceDirs.has(normalizePosixRelPath(directory))
}

export function collectAllowedPeerProviders (
  lockfile: LockfileObject,
  scope: NormalizedExoticSubdepsScope,
  workspaceDirs: Set<string>
): Set<DepPath> {
  const providers = new Set<DepPath>()
  for (const [, importer] of trustedImporterEntries(lockfile, scope)) {
    addProvidersFromDeps(providers, importer.dependencies)
    addProvidersFromDeps(providers, importer.devDependencies)
    addProvidersFromDeps(providers, importer.optionalDependencies)
  }
  for (const snapshot of Object.values(lockfile.packages ?? {}) as PackageSnapshot[]) {
    if (isWorkspaceSnapshot(snapshot, workspaceDirs)) {
      addProvidersFromDeps(providers, snapshot.dependencies)
      addProvidersFromDeps(providers, snapshot.optionalDependencies)
    }
  }
  return providers
}

function addProvidersFromDeps (providers: Set<DepPath>, deps: ResolvedDependencies | undefined): void {
  if (deps == null) return
  for (const [alias, ref] of Object.entries(deps)) {
    if (typeof ref !== 'string') continue
    const depPath = dp.refToRelative(ref, alias)
    if (depPath != null) providers.add(depPath)
  }
}
