import fs from 'node:fs'
import path from 'node:path'

import { resolveFromCatalog } from '@pnpm/catalogs.resolver'
import type { Catalogs } from '@pnpm/catalogs.types'
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
import type { DirectoryResolution } from '@pnpm/resolving.resolver-base'
import type { DependencyManifest, ReadPackageHook } from '@pnpm/types'
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
  resolveSpecPath,
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
}

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
 * Whether the dependencies of a local directory dependency still match its
 * lockfile snapshot. The package's children and the versions they accept are
 * the resolver's (see `getNonDevWantedDependencies` and
 * `lockedVersionSatisfies`), read from its manifest after the read-package
 * hook. `parentPkgAliases` are the dependencies of the project that depends on
 * it, which supply its peers.
 */
export async function localDirectoryDepIsUpToDate (
  ctx: LocalDirectoryDepContext,
  pkgSnapshot: PackageSnapshot,
  parentPkgAliases: Record<string, unknown> = {}
): Promise<boolean> {
  if (!('directory' in (pkgSnapshot.resolution ?? {}))) return true
  const localDepDir = path.join(ctx.lockfileDir, (pkgSnapshot.resolution as DirectoryResolution).directory)
  const manifest = await readHookedManifestOnce(ctx, localDepDir)
  if (!manifest) return false
  if (!recordedPeersMatch(ctx, manifest, pkgSnapshot)) return false
  const children = getNonDevWantedDependencies(prepareDependencyManifest(clone(manifest), {
    autoInstallPeers: ctx.resolution?.autoInstallPeers ?? true,
    parentPkgAliases,
  }))
  if (!recordedChildrenAreExpected(manifest, children, pkgSnapshot)) return false
  const localDep: LocalDirectoryDep = {
    lockfileDir: ctx.lockfileDir,
    localDepDir,
    manifestsByDir: ctx.manifestsByDir,
    workspaceRoot: ctx.workspaceDir ?? ctx.lockfileDir,
  }
  return pEvery.default(children, async (child) => childIsUpToDate(ctx, localDep, pkgSnapshot, child))
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
  child: ManifestWantedDependency
): Promise<boolean> {
  const recorded = child.optional ? pkgSnapshot.optionalDependencies : pkgSnapshot.dependencies
  const lockfileDep = recorded?.[child.alias]
  if (!lockfileDep) {
    // A child recorded as required has changed, not gone unresolved.
    return child.optional && ctx.resolution?.frozenLockfile === true &&
      !pkgSnapshot.dependencies?.[child.alias]
  }
  const currentSpec = resolveCatalogSpec(ctx, child)
  if (currentSpec == null) return false
  if (isLocalSpec(currentSpec)) {
    return localSpecIsUpToDate(localDep, { depName: child.alias, currentSpec, lockfileDep })
  }
  const locked = lockedPackage(ctx, child.alias, lockfileDep)
  if (locked == null) return false
  if (isLockedToOtherSource(currentSpec, locked.depPath)) return false
  return lockedVersionSatisfies({ alias: child.alias, bareSpecifier: currentSpec }, locked) ??
    (ctx.resolution?.frozenLockfile === true && locked.name === getTargetPkgName(currentSpec, child.alias))
}

function isLocalSpec (spec: string): boolean {
  return spec.startsWith('link:') || spec.startsWith('file:') || spec.startsWith('workspace:')
}

/**
 * Whether a spec that names one exact source, a tarball URL or a git commit,
 * asks for a source other than the one the package is locked to. The lockfile
 * keys such a package by the URL or commit it resolved.
 */
function isLockedToOtherSource (spec: string, depPath: string): boolean {
  const lockedId = removeSuffix(depPath)
  const git = parseGitSpecCommittishAndPath(spec)
  if (git !== undefined) {
    return git.gitCommittish != null && /^[0-9a-f]{40}$/i.test(git.gitCommittish) &&
      !isLockedToGitCommit(lockedId, { commit: git.gitCommittish, path: git.path })
  }
  if (spec.startsWith('http:') || spec.startsWith('https:')) {
    return !URL.canParse(spec) || !lockedId.endsWith(`@${new URL(spec).toString()}`)
  }
  return false
}

/** Whether a locked git or hosted-archive id names the commit and subdirectory. */
function isLockedToGitCommit (lockedId: string, { commit, path }: { commit: string, path?: string }): boolean {
  const pathIndex = lockedId.lastIndexOf('path:')
  const lockedSource = pathIndex === -1 ? lockedId : lockedId.slice(0, pathIndex)
  const lockedPath = pathIndex === -1 ? undefined : lockedId.slice(pathIndex + 'path:'.length)
  return path === lockedPath && lockedSource.toLowerCase().split(/[#&/]/).includes(commit.toLowerCase())
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
      resolveSpecPath(localDep.localDepDir, currentSpec.slice(5)) === resolveSpecPath(localDep.lockfileDir, lockfileDep.slice(5))
    )
  }
  const cleanLockfileDep = removeSuffix(lockfileDep)
  const lockfilePath = getLocalPath(cleanLockfileDep)
  if (currentSpec.startsWith('file:')) {
    return localPathMatches(localDep, currentSpec.slice(5), lockfilePath)
  }
  return workspaceDependencyIsUpToDate(localDep, { depName, currentSpec, cleanLockfileDep, lockfilePath })
}

function localPathMatches (localDep: LocalDirectoryDep, specPath: string, lockfilePath: string | null): boolean {
  return lockfilePath != null &&
    resolveSpecPath(localDep.localDepDir, specPath) === resolveSpecPath(localDep.lockfileDir, lockfilePath)
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
  const target = currentSpec.slice(10)
  if (isWorkspacePath(target)) {
    return localPathMatches(localDep, target, lockfilePath)
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
  return range === '*' || range === '^' || range === '~' || range === '' ||
    semver.satisfies(targetPkg.version, range, { loose: true })
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
