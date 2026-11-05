import fs from 'node:fs'
import path from 'node:path'

import { parseCatalogProtocol } from '@pnpm/catalogs.protocol-parser'
import { resolveFromCatalog } from '@pnpm/catalogs.resolver'
import type { Catalogs } from '@pnpm/catalogs.types'
import { DEFAULT_REGISTRIES_BY_SCOPE } from '@pnpm/config.normalize-registries'
import { refToRelative, removeSuffix } from '@pnpm/deps.path'
import {
  getNonDevWantedDependencies,
  getPeerSet,
  lockedVersionSatisfies,
  type ManifestWantedDependency,
  prepareDependencyManifest,
} from '@pnpm/installing.deps-resolver'
import type { PackageSnapshot, PackageSnapshots } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { safeReadPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import { parseGitSpecCommittishAndPath } from '@pnpm/resolving.git-resolver'
import { isLocalFilesystemSpecifier, localSpecPath, recordedLocalPath } from '@pnpm/resolving.local-resolver'
import type { DirectoryResolution } from '@pnpm/resolving.resolver-base'
import type { DependencyManifest, ReadPackageHook, RegistryContext } from '@pnpm/types'
import { safeReadParentPublishManifest, safeReadProjectManifestOnly } from '@pnpm/workspace.project-manifest-reader'
import pEvery from 'p-every'
import { pathExists } from 'path-exists'
import { clone } from 'ramda'
import semver from 'semver'

import {
  getDepActualName,
  getDepVersion,
  getLocalPath,
  getTargetPkgName,
  getVersionRange,
  isSubdirectory,
  isWorkspacePath,
  versionSatisfies,
} from './dependencySpec.js'

export interface LocalDirectoryDepContext {
  lockfileDir: string
  lockfilePackages?: PackageSnapshots
  manifestsByDir: Record<string, DependencyManifest>
  workspaceDir?: string
  /** The settings and hooks the lockfile was resolved with. */
  resolution?: LocalDirectoryResolution
  /**
   * Each directory's manifest after the read-package hook, shared by the
   * projects of one check. A caller creates it per check, so a later check
   * reads the manifests again.
   */
  hookedManifests?: Map<string, Promise<DependencyManifest | undefined>>
}

export interface LocalDirectoryResolution {
  autoInstallPeers?: boolean
  catalogs?: Catalogs
  readPackageHook?: ReadPackageHook
  /**
   * The explicit `--frozen-lockfile` install, which never resolves. It keeps
   * what the lockfile records and the check cannot verify: an optional
   * dependency the lockfile could not resolve, and a dependency whose spec is
   * not a range, such as a tag or a git or tarball spec, since the lockfile
   * does not record the spec it resolved. Any other install resolves those
   * again.
   */
  frozenLockfile?: boolean
  /**
   * How the registry of a locked child is decided, which a locked child must
   * share with its spec. The default registries, which declare no prefix,
   * when absent.
   */
  registries?: RegistryContext
}

const DEFAULT_REGISTRY_CONTEXT: RegistryContext = { registriesByScope: DEFAULT_REGISTRIES_BY_SCOPE }

interface LocalDirectoryDep {
  lockfileDir: string
  localDepDir: string
  manifestsByDir: Record<string, DependencyManifest>
  workspaceRoot: string
}

interface DependencyOfLocalDirectoryDep {
  depName: string
  currentSpec: string
  lockfileDep: string
}

/**
 * Creates a check of whether the dependencies of a local directory dependency
 * still match its lockfile snapshot. The package's children and the versions
 * they accept are the resolver's (see `getNonDevWantedDependencies` and
 * `lockedVersionSatisfies`), read from its manifest after the read-package
 * hook. `parentPkgAliases` are the dependencies of the project that depends on
 * it, which supply its peers.
 *
 * The checks of one checker share the snapshots they visited, so a snapshot
 * reached through several direct dependencies is walked once per peer scope.
 * Create one checker per project.
 */
export function createLocalDirectoryDepUpToDateChecker (
  ctx: LocalDirectoryDepContext
): (pkgSnapshot: PackageSnapshot, parentPkgAliases: Record<string, unknown>, declaration?: Declaration) => Promise<boolean> {
  const visited = new Map<PackageSnapshot, Set<string>>()
  // Collected on the first check, since most projects have no local directory dependency to check.
  let recordedPeers: string[] | undefined
  return async (pkgSnapshot, parentPkgAliases, declaration = { declaredByPath: false }) => localDirectoryDepIsUpToDateRec(ctx, pkgSnapshot, {
    ...declaration,
    parentPkgAliases,
    visited,
    recordedPeers: recordedPeers ??= recordedPeersOf(ctx.lockfilePackages),
  })
}

/** How the package being checked was declared. */
export interface Declaration {
  /**
   * Whether a path spec, such as `file:../lib` or `../lib`, declared the
   * package. The resolver leaves the `catalog:` specs of such a package's
   * children unresolved.
   */
  declaredByPath: boolean
  /**
   * The lockfile does not record which spec declared the package, so it is
   * checked under both declarations. Only the workspace one judges its
   * `catalog:` children then.
   */
  declarationAssumed?: boolean
}

interface DirectoryWalk extends Declaration {
  /** The aliases of every ancestor, which supply the package's peers. */
  parentPkgAliases: Record<string, unknown>
  /** The peer scopes each snapshot was checked in. */
  visited: Map<PackageSnapshot, Set<string>>
  /**
   * Every peer the lockfile records for a package, which bounds the part of
   * a scope the descendants of a package can take.
   */
  recordedPeers: string[]
}

const recordedPeersByPackages = new WeakMap<object, string[]>()

function recordedPeersOf (lockfilePackages: LocalDirectoryDepContext['lockfilePackages']): string[] {
  if (lockfilePackages == null) return []
  let peers = recordedPeersByPackages.get(lockfilePackages)
  if (peers == null) {
    peers = Array.from(new Set(Object.values(lockfilePackages).flatMap((pkg) => Object.keys(pkg.peerDependencies ?? {}))))
    recordedPeersByPackages.set(lockfilePackages, peers)
  }
  return peers
}

async function localDirectoryDepIsUpToDateRec (
  ctx: LocalDirectoryDepContext,
  pkgSnapshot: PackageSnapshot,
  walk: DirectoryWalk
): Promise<boolean> {
  if (!('directory' in (pkgSnapshot.resolution ?? {}))) return true
  const localDepDir = path.join(ctx.lockfileDir, (pkgSnapshot.resolution as DirectoryResolution).directory)
  const manifest = await readHookedManifestOnce(ctx, localDepDir)
  if (!manifest) return false
  // A revisit in the same peer scope is a cycle or a shared dependency that
  // is already being checked.
  if (!markVisited(walk, pkgSnapshot, manifest)) return true
  if (!recordedPeersMatch(ctx, manifest, pkgSnapshot)) return false
  const children = getNonDevWantedDependencies(prepareDependencyManifest(clone(manifest), {
    autoInstallPeers: ctx.resolution?.autoInstallPeers ?? true,
    parentPkgAliases: walk.parentPkgAliases,
  }))
  if (!recordedChildrenAreExpected(manifest, children, pkgSnapshot)) return false
  const localDep: LocalDirectoryDep = {
    lockfileDir: ctx.lockfileDir,
    localDepDir,
    manifestsByDir: ctx.manifestsByDir,
    workspaceRoot: ctx.workspaceDir ?? ctx.lockfileDir,
  }
  const childWalk: DirectoryWalk = {
    declaredByPath: walk.declaredByPath,
    declarationAssumed: walk.declarationAssumed,
    parentPkgAliases: { ...walk.parentPkgAliases, ...dependencyAliasesOf(pkgSnapshot) },
    visited: walk.visited,
    recordedPeers: walk.recordedPeers,
  }
  return pEvery.default(children, async (child) => childIsUpToDate(ctx, localDep, pkgSnapshot, child, childWalk))
}

/** Records the snapshot in its peer scope. `false` when it was already there. */
function markVisited (walk: DirectoryWalk, pkgSnapshot: PackageSnapshot, manifest: DependencyManifest): boolean {
  const scope = Array.from(new Set([...Object.keys(manifest.peerDependencies ?? {}), ...walk.recordedPeers]))
    .filter((peer) => Boolean(walk.parentPkgAliases[peer]))
    .sort()
    .concat(walk.declaredByPath ? ['\0path'] : [], walk.declarationAssumed ? ['\0assumed'] : [])
    .join('\0')
  let scopes = walk.visited.get(pkgSnapshot)
  if (scopes == null) {
    scopes = new Set()
    walk.visited.set(pkgSnapshot, scopes)
  }
  if (scopes.has(scope)) return false
  scopes.add(scope)
  return true
}

function dependencyAliasesOf (pkgSnapshot: PackageSnapshot): Record<string, true> {
  return Object.fromEntries([
    ...Object.keys(pkgSnapshot.dependencies ?? {}),
    ...Object.keys(pkgSnapshot.optionalDependencies ?? {}),
  ].map((alias) => [alias, true as const]))
}

/**
 * The manifest the resolver describes a directory with. A directory without a
 * manifest is described by the project that publishes from it, else by its
 * name alone. `undefined` when the directory is missing.
 */
async function readDirectoryManifest (dir: string): Promise<DependencyManifest | undefined> {
  const manifest = await safeReadProjectManifestOnly(dir) as DependencyManifest | null
  if (manifest != null) return manifest
  if (!await pathExists(dir)) return undefined
  return (await safeReadParentPublishManifest(dir) as DependencyManifest | null) ?? { name: path.basename(dir) } as DependencyManifest
}

async function readHookedManifestOnce (ctx: LocalDirectoryDepContext, dir: string): Promise<DependencyManifest | undefined> {
  if (ctx.hookedManifests == null) return readHookedManifest(ctx, dir)
  let manifest = ctx.hookedManifests.get(dir)
  if (manifest == null) {
    manifest = readHookedManifest(ctx, dir)
    ctx.hookedManifests.set(dir, manifest)
  }
  return manifest
}

async function readHookedManifest (ctx: LocalDirectoryDepContext, dir: string): Promise<DependencyManifest | undefined> {
  const declared = ctx.manifestsByDir[dir] ?? await readDirectoryManifest(dir)
  if (!declared) return undefined
  return applyReadPackageHook(ctx, declared)
}

/**
 * The read-package hook is called the way the resolver calls it, without a
 * directory. A hook that returns nothing leaves the manifest unchanged.
 */
async function applyReadPackageHook (ctx: LocalDirectoryDepContext, declared: DependencyManifest): Promise<DependencyManifest> {
  const hook = ctx.resolution?.readPackageHook
  if (hook == null) return declared
  return (await hook(clone(declared)) as DependencyManifest | undefined) ?? declared
}

/**
 * The lockfile records the peers the package declares. A lockfile written
 * before it recorded every declared peer records only the peers taken without
 * any shadowing, the ones that are not also `dependencies`.
 */
function recordedPeersMatch (ctx: LocalDirectoryDepContext, manifest: DependencyManifest, pkgSnapshot: PackageSnapshot): boolean {
  const { declared, taken } = getPeerSet(manifest)
  const recorded = pkgSnapshot.peerDependencies ?? {}
  return Object.keys(taken).every((name) => Object.hasOwn(recorded, name)) &&
    Object.keys(recorded).every((name) => {
      const peer = declared[name]
      return peer != null &&
        peerRangeMatches(ctx, { alias: name, bareSpecifier: peer.version }, recorded[name]) &&
        Boolean(peer.optional) === Boolean(pkgSnapshot.peerDependenciesMeta?.[name]?.optional)
    })
}

function peerRangeMatches (
  ctx: LocalDirectoryDepContext,
  peer: { alias: string, bareSpecifier: string },
  recordedRange: string | undefined
): boolean {
  if (recordedRange == null) return false
  return recordedRange === peer.bareSpecifier || recordedRange === resolveCatalogSpec(ctx, peer)
}

/** Every child the snapshot records is one the resolver walks, or a peer, whose resolution it records too. */
function recordedChildrenAreExpected (
  manifest: DependencyManifest,
  children: ManifestWantedDependency[],
  pkgSnapshot: PackageSnapshot
): boolean {
  const expected = new Set([
    ...children.map(({ alias }) => alias),
    ...Object.keys(getPeerSet(manifest).declared),
    ...Object.keys(manifest.peerDependencies ?? {}),
  ])
  return [...Object.keys(pkgSnapshot.dependencies ?? {}), ...Object.keys(pkgSnapshot.optionalDependencies ?? {})]
    .every((name) => expected.has(name))
}

async function childIsUpToDate (
  ctx: LocalDirectoryDepContext,
  localDep: LocalDirectoryDep,
  pkgSnapshot: PackageSnapshot,
  child: ManifestWantedDependency,
  walk: DirectoryWalk
): Promise<boolean> {
  const recorded = child.optional ? pkgSnapshot.optionalDependencies : pkgSnapshot.dependencies
  const lockfileDep = recorded?.[child.alias]
  if (!lockfileDep) {
    // A child recorded as required has changed, not gone unresolved.
    return child.optional && ctx.resolution?.frozenLockfile === true &&
      !pkgSnapshot.dependencies?.[child.alias]
  }
  const currentSpec = resolvedChildSpec(ctx, child, walk)
  if (currentSpec == null) return false
  if (isLocalSpec(currentSpec)) {
    return localChildIsUpToDate(ctx, localDep, pkgSnapshot, child, currentSpec, lockfileDep, walk)
  }
  const locked = lockedPackage(ctx, child.alias, lockfileDep)
  if (locked == null) return false
  if (isLockedToOtherSource(currentSpec, locked)) return false
  return lockedVersionSatisfies({ alias: child.alias, bareSpecifier: currentSpec }, locked, ctx.resolution?.registries ?? DEFAULT_REGISTRY_CONTEXT) ??
    (ctx.resolution?.frozenLockfile === true && locked.name === getTargetPkgName(currentSpec, child.alias))
}

/**
 * The spec the resolver resolves `child` with. `undefined` when it resolves
 * none: a missing catalog entry, or a `catalog:` spec of a child of a package
 * declared by path, which the resolver leaves as written and no resolver
 * accepts.
 */
function resolvedChildSpec (ctx: LocalDirectoryDepContext, child: ManifestWantedDependency, walk: DirectoryWalk): string | undefined {
  if (!walk.declaredByPath || walk.declarationAssumed) return resolveCatalogSpec(ctx, child)
  return parseCatalogProtocol(child.bareSpecifier) == null ? child.bareSpecifier : undefined
}

/**
 * Whether a `link:`, `file:` or `workspace:` child still resolves to the
 * recorded directory, and that directory's own dependencies still match its
 * lockfile snapshot.
 */
async function localChildIsUpToDate (
  ctx: LocalDirectoryDepContext,
  localDep: LocalDirectoryDep,
  pkgSnapshot: PackageSnapshot,
  child: ManifestWantedDependency,
  currentSpec: string,
  lockfileDep: string,
  walk: DirectoryWalk
): Promise<boolean> {
  if (!await localSpecIsUpToDate(localDep, { depName: child.alias, currentSpec, lockfileDep })) return false
  // A directory child whose spec still matches can itself be stale: its own
  // dependencies may have changed without the directory moving. Links have
  // no dependency snapshot to validate, so only directory children recurse.
  const depPath = refToRelative(lockfileDep, child.alias)
  if (depPath == null) return true
  const childSnapshot = ctx.lockfilePackages?.[depPath]
  if (childSnapshot == null) {
    // A workspace child recorded through a file:-resolved parent has no
    // snapshot of its own; the workspace verifies it through its importer
    // entry. A file: child with no snapshot has an incomplete lockfile
    // entry: its own dependencies cannot be validated, so it is stale.
    return currentSpec.startsWith('workspace:')
  }
  return localDirectoryDepIsUpToDateRec(ctx, childSnapshot, {
    ...walk,
    declaredByPath: isLocalFilesystemSpecifier(child.bareSpecifier),
    declarationAssumed: false,
  })
}

function isLocalSpec (spec: string): boolean {
  return spec.startsWith('link:') || spec.startsWith('file:') || spec.startsWith('workspace:')
}

/**
 * Whether a spec that names one exact source, a tarball URL or a git commit,
 * asks for a source other than the one the package is locked to. The lockfile
 * keys such a package by the URL or commit it resolved.
 */
function isLockedToOtherSource (spec: string, { depPath, name }: { depPath: string, name: string }): boolean {
  const lockedId = removeSuffix(depPath)
  const git = parseGitSpecCommittishAndPath(spec)
  if (git !== undefined) {
    return git.gitCommittish != null && isFullCommit(git.gitCommittish) &&
      !isLockedToGitCommit(lockedId, { commit: git.gitCommittish, path: git.path })
  }
  if (spec.startsWith('http:') || spec.startsWith('https:')) {
    // A URL may end in a parenthesized segment, which removeSuffix takes for a peer suffix.
    return !URL.canParse(spec) || ![depPath, lockedId].includes(`${name}@${new URL(spec).toString()}`)
  }
  return false
}

/** Whether a locked git or hosted-archive id names the commit and subdirectory. */
function isLockedToGitCommit (lockedId: string, { commit, path }: { commit: string, path?: string }): boolean {
  const pathIndex = lockedId.lastIndexOf('path:')
  const lockedSource = pathIndex === -1 ? lockedId : lockedId.slice(0, pathIndex).replace(/[#&]$/, '')
  const lockedPath = pathIndex === -1 ? undefined : lockedId.slice(pathIndex + 'path:'.length)
  // Hosted archive ids end in the commit or in `<sha>.tar.gz` or `<project>-<sha>.tar.gz` after it.
  const lockedCommit = lockedSource.split(/[#/]/).map((segment) => segment.replace(/\.tar\.gz$/, '')).filter(isFullCommit).at(-1)
  return path === lockedPath && lockedCommit?.toLowerCase() === commit.toLowerCase()
}

function isFullCommit (committish: string): boolean {
  return /^[0-9a-f]{40}$/i.test(committish)
}

/** The dependency path, name, version and registry of the package a dependency is locked to. */
function lockedPackage (
  ctx: LocalDirectoryDepContext,
  alias: string,
  lockfileDep: string
): { depPath: string, name: string, version: string, registryName?: string } | undefined {
  const depPath = refToRelative(lockfileDep, alias)
  const locked = depPath == null ? undefined : ctx.lockfilePackages?.[depPath]
  return depPath == null || locked == null ? undefined : { depPath, ...nameVerFromPkgSnapshot(depPath, locked) }
}

/**
 * The spec with a `catalog:` reference replaced by the catalog's entry, as the
 * resolver resolves every dependency's. `undefined` when the catalog has no
 * entry for it.
 */
function resolveCatalogSpec (ctx: LocalDirectoryDepContext, dep: { alias: string, bareSpecifier: string }): string | undefined {
  const result = resolveFromCatalog(ctx.resolution?.catalogs ?? {}, dep)
  switch (result.type) {
    case 'found': return result.resolution.specifier
    case 'unused': return dep.bareSpecifier
    default: return undefined
  }
}

/** Whether a `link:`, `file:` or `workspace:` spec still resolves to the recorded dependency. */
async function localSpecIsUpToDate (
  localDep: LocalDirectoryDep,
  { depName, currentSpec, lockfileDep }: DependencyOfLocalDirectoryDep
): Promise<boolean> {
  if (currentSpec.startsWith('link:')) {
    return (
      lockfileDep.startsWith('link:') &&
      localSpecPath(currentSpec, localDep.localDepDir) === recordedLocalPath(lockfileDep.slice(5), localDep.lockfileDir)
    )
  }
  const cleanLockfileDep = removeSuffix(lockfileDep)
  const lockfilePath = getLocalPath(cleanLockfileDep)
  if (currentSpec.startsWith('file:')) {
    return localPathMatches(localDep, currentSpec, lockfilePath)
  }
  return workspaceDependencyIsUpToDate(localDep, { depName, currentSpec, cleanLockfileDep, lockfilePath })
}

function localPathMatches (localDep: LocalDirectoryDep, currentSpec: string, lockfilePath: string | null): boolean {
  return lockfilePath != null &&
    localSpecPath(currentSpec, localDep.localDepDir) === recordedLocalPath(lockfilePath, localDep.lockfileDir)
}

function hasExpectedName (cleanLockfileDep: string, { depName, currentSpec }: { depName: string, currentSpec: string }): boolean {
  const expectedName = getTargetPkgName(currentSpec, depName)
  const actualName = getDepActualName(cleanLockfileDep, depName)
  return actualName === expectedName
}

async function workspaceDependencyIsUpToDate (
  localDep: LocalDirectoryDep,
  { depName, currentSpec, cleanLockfileDep, lockfilePath }: {
    depName: string
    currentSpec: string
    cleanLockfileDep: string
    lockfilePath: string | null
  }
): Promise<boolean> {
  if (isWorkspacePath(currentSpec.slice(10))) {
    return localPathMatches(localDep, currentSpec, lockfilePath)
  }
  const range = getVersionRange(currentSpec)
  if (lockfilePath != null) {
    return linkedWorkspaceTargetSatisfies(localDep, { depName, currentSpec, range, lockfilePath })
  }
  if (!hasExpectedName(cleanLockfileDep, { depName, currentSpec })) {
    return false
  }
  const lockfileVersion = getDepVersion(cleanLockfileDep)
  return !semver.valid(lockfileVersion) || semver.satisfies(lockfileVersion, range, { loose: true })
}

async function linkedWorkspaceTargetSatisfies (
  localDep: LocalDirectoryDep,
  { depName, currentSpec, range, lockfilePath }: {
    depName: string
    currentSpec: string
    range: string
    lockfilePath: string
  }
): Promise<boolean> {
  if (path.isAbsolute(lockfilePath)) {
    return false
  }
  const targetDir = path.resolve(localDep.lockfileDir, lockfilePath)
  if (!isSubdirectory(localDep.workspaceRoot, targetDir)) {
    return false
  }
  if (!await realPathStaysInsideWorkspace(localDep.workspaceRoot, targetDir)) {
    return false
  }
  const targetPkg = localDep.manifestsByDir[targetDir] ?? await safeReadPackageJsonFromDir(targetDir)
  const expectedName = getTargetPkgName(currentSpec, depName)
  if (!targetPkg || targetPkg.name !== expectedName) {
    return false
  }
  return versionSatisfies(targetPkg.version, range)
}

async function realPathStaysInsideWorkspace (workspaceRoot: string, targetDir: string): Promise<boolean> {
  let realTargetDir: string
  let realWorkspaceRoot: string
  let realManifestPath: string
  try {
    [realTargetDir, realWorkspaceRoot, realManifestPath] = await Promise.all([
      fs.promises.realpath(targetDir),
      fs.promises.realpath(workspaceRoot),
      fs.promises.realpath(path.join(targetDir, 'package.json')),
    ])
  } catch {
    return false
  }
  return isSubdirectory(realWorkspaceRoot, realTargetDir) && isSubdirectory(realWorkspaceRoot, realManifestPath)
}
