import fs from 'node:fs'
import path from 'node:path'

import { resolveFromCatalog } from '@pnpm/catalogs.resolver'
import type { Catalogs } from '@pnpm/catalogs.types'
import { refToRelative, removeSuffix } from '@pnpm/deps.path'
import {
  getNonDevWantedDependencies,
  lockedVersionSatisfies,
  type ManifestWantedDependency,
  peerDependenciesWithoutOwn,
  prepareDependencyManifest,
} from '@pnpm/installing.deps-resolver'
import type { PackageSnapshot, PackageSnapshots } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { safeReadPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import type { DirectoryResolution } from '@pnpm/resolving.resolver-base'
import type { DependencyManifest, ReadPackageHook } from '@pnpm/types'
import { safeReadParentPublishManifest } from '@pnpm/workspace.project-manifest-reader'
import pEvery from 'p-every'
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
}

export interface LocalDirectoryResolution {
  autoInstallPeers?: boolean
  catalogs?: Catalogs
  readPackageHook?: ReadPackageHook
  /** A frozen install keeps the optional dependencies the lockfile could not resolve. */
  allowUnresolvedOptionalDependencies?: boolean
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
  const declared = ctx.manifestsByDir[localDepDir] ?? await readDirectoryManifest(localDepDir)
  if (!declared) return false
  const manifest = await readHookedManifest(ctx, declared)
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
 * `package.json` is described by the project that publishes from it, else by
 * its name alone. `undefined` when the directory is missing.
 */
async function readDirectoryManifest (dir: string): Promise<DependencyManifest | undefined> {
  const manifest = await safeReadPackageJsonFromDir(dir)
  if (manifest != null) return manifest
  if (!fs.existsSync(dir)) return undefined
  return (await safeReadParentPublishManifest(dir) as DependencyManifest | null) ?? { name: path.basename(dir) } as DependencyManifest
}

/** The read-package hook is called the way the resolver calls it, without a directory. */
async function readHookedManifest (ctx: LocalDirectoryDepContext, declared: DependencyManifest): Promise<DependencyManifest> {
  const hook = ctx.resolution?.readPackageHook
  return hook == null ? declared : await hook(clone(declared)) as DependencyManifest
}

/** The lockfile records the peers the resolver reads from the manifest (see `peerDependenciesWithoutOwn`). */
function recordedPeersMatch (ctx: LocalDirectoryDepContext, manifest: DependencyManifest, pkgSnapshot: PackageSnapshot): boolean {
  const peers = Object.entries(peerDependenciesWithoutOwn(manifest))
  const recorded = pkgSnapshot.peerDependencies ?? {}
  if (peers.length !== Object.keys(recorded).length) return false
  return peers.every(([name, peer]) =>
    peerRangeMatches(ctx, { alias: name, bareSpecifier: peer.version }, recorded[name]) &&
    Boolean(peer.optional) === Boolean(pkgSnapshot.peerDependenciesMeta?.[name]?.optional)
  )
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
    ...Object.keys(peerDependenciesWithoutOwn(manifest)),
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
    return child.optional && ctx.resolution?.allowUnresolvedOptionalDependencies === true &&
      !pkgSnapshot.dependencies?.[child.alias]
  }
  const currentSpec = resolveCatalogSpec(ctx, child)
  if (currentSpec == null) return false
  if (isLocalSpec(currentSpec)) {
    return localSpecIsUpToDate(localDep, { depName: child.alias, currentSpec, lockfileDep })
  }
  const locked = lockedPackage(ctx, child.alias, lockfileDep)
  if (locked == null) return false
  return lockedVersionSatisfies({ alias: child.alias, bareSpecifier: currentSpec }, locked) ??
    getDepActualName(removeSuffix(lockfileDep), child.alias) === getTargetPkgName(currentSpec, child.alias)
}

function isLocalSpec (spec: string): boolean {
  return spec.startsWith('link:') || spec.startsWith('file:') || spec.startsWith('workspace:')
}

/** The name, version and registry of the package a dependency is locked to. */
function lockedPackage (
  ctx: LocalDirectoryDepContext,
  alias: string,
  lockfileDep: string
): { name: string, version: string, registryName?: string } | undefined {
  const depPath = refToRelative(lockfileDep, alias)
  const locked = depPath == null ? undefined : ctx.lockfilePackages?.[depPath]
  return depPath == null || locked == null ? undefined : nameVerFromPkgSnapshot(depPath, locked)
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
