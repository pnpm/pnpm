import fs from 'node:fs'
import path from 'node:path'

import { resolveFromCatalog } from '@pnpm/catalogs.resolver'
import type { Catalogs } from '@pnpm/catalogs.types'
import { removeSuffix } from '@pnpm/deps.path'
import type { PackageSnapshot } from '@pnpm/lockfile.types'
import { safeReadPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import type { DirectoryResolution } from '@pnpm/resolving.resolver-base'
import {
  DEPENDENCIES_OR_PEER_FIELDS,
  type DependencyManifest,
  type PeerDependenciesMeta,
  type ReadPackageHook,
} from '@pnpm/types'
import pEvery from 'p-every'
import { clone, pickBy } from 'ramda'
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
  /**
   * The catalogs that `catalog:` specs in the local directory dependency's
   * manifest resolve from.
   */
  catalogs?: Catalogs
  lockfileDir: string
  manifestsByDir: Record<string, DependencyManifest>
  /**
   * The hook the resolver applied to the local directory dependency's
   * manifest, so that the dependencies are compared the way the resolver saw
   * them (with overrides and package extensions applied).
   */
  readPackageHook?: ReadPackageHook
  workspaceDir?: string
}

interface LocalDirectoryDep {
  catalogs?: Catalogs
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
 * lockfile snapshot.
 */
export async function localDirectoryDepIsUpToDate (
  ctx: LocalDirectoryDepContext,
  pkgSnapshot: PackageSnapshot
): Promise<boolean> {
  if (!('directory' in (pkgSnapshot.resolution ?? {}))) return true
  const localDepDir = path.join(ctx.lockfileDir, (pkgSnapshot.resolution as DirectoryResolution).directory)
  const manifest = await readLocalDirectoryDepManifest(ctx, localDepDir)
  if (!manifest) return false
  if (!peerDependenciesMetaMatch(manifest.peerDependenciesMeta ?? {}, pkgSnapshot.peerDependenciesMeta ?? {})) {
    return false
  }
  const localDep: LocalDirectoryDep = {
    catalogs: ctx.catalogs,
    lockfileDir: ctx.lockfileDir,
    localDepDir,
    manifestsByDir: ctx.manifestsByDir,
    workspaceRoot: ctx.workspaceDir ?? ctx.lockfileDir,
  }
  const lockfilePeers = pkgSnapshot.peerDependencies ?? {}
  const peerDependencies = getPeersResolvedAsPeers(manifest, lockfilePeers)
  if (!peerRangesMatch(localDep, peerDependencies, lockfilePeers)) {
    return false
  }
  return pEvery.default(DEPENDENCIES_OR_PEER_FIELDS, async (depField) => {
    if (depField === 'devDependencies' || depField === 'peerDependencies') return true
    return depFieldIsUpToDate(localDep, {
      manifestDeps: manifest[depField] ?? {},
      lockfileDeps: pkgSnapshot[depField] ?? {},
      peerDependencies,
    })
  })
}

async function readLocalDirectoryDepManifest (ctx: LocalDirectoryDepContext, localDepDir: string): Promise<DependencyManifest | null> {
  const manifest = ctx.manifestsByDir[localDepDir] ?? await safeReadPackageJsonFromDir(localDepDir)
  if (manifest == null || ctx.readPackageHook == null) return manifest
  return ctx.readPackageHook(clone(manifest), localDepDir)
}

/**
 * A dependency also declared as a peer resolves as a peer when the lockfile
 * records it among the peers, and as an own dependency otherwise. The
 * resolver also resolves the optional dependencies and the package's own
 * name as own dependencies.
 */
function getPeersResolvedAsPeers (manifest: DependencyManifest, lockfilePeers: Record<string, string>): Record<string, string> {
  return pickBy((_, peerName) =>
    peerName !== manifest.name &&
    !Object.hasOwn(manifest.optionalDependencies ?? {}, peerName) &&
    (!Object.hasOwn(manifest.dependencies ?? {}, peerName) || Object.hasOwn(lockfilePeers, peerName)),
  manifest.peerDependencies ?? {})
}

/**
 * The snapshot of a package records its peer dependency ranges as the
 * manifest declares them, with `catalog:` specs resolved.
 */
function peerRangesMatch (
  localDep: LocalDirectoryDep,
  manifestPeers: Record<string, string>,
  lockfilePeers: Record<string, string>
): boolean {
  const manifestPeerNames = Object.keys(manifestPeers)
  return manifestPeerNames.length === Object.keys(lockfilePeers).length &&
    manifestPeerNames.every((peerName) => {
      const lockfileRange = lockfilePeers[peerName]
      if (lockfileRange == null) return false
      const range = manifestPeers[peerName]
      return range === lockfileRange || resolveCatalogSpec(localDep, peerName, range) === lockfileRange
    })
}

function peerDependenciesMetaMatch (
  manifestPeerMeta: PeerDependenciesMeta,
  lockfilePeerMeta: PeerDependenciesMeta
): boolean {
  return optionalPeersAgree(manifestPeerMeta, lockfilePeerMeta) && optionalPeersAgree(lockfilePeerMeta, manifestPeerMeta)
}

function optionalPeersAgree (peerMeta: PeerDependenciesMeta, otherPeerMeta: PeerDependenciesMeta): boolean {
  return Object.entries(peerMeta).every(([name, meta]) => Boolean(meta?.optional) === Boolean(otherPeerMeta[name]?.optional))
}

/**
 * The snapshot's dependencies also hold the resolved peer dependencies, whose
 * ranges `peerRangesMatch` compares.
 */
async function depFieldIsUpToDate (
  localDep: LocalDirectoryDep,
  { manifestDeps, lockfileDeps, peerDependencies }: {
    manifestDeps: Record<string, string>
    lockfileDeps: Record<string, string>
    peerDependencies: Record<string, string>
  }
): Promise<boolean> {
  if (Object.keys(lockfileDeps).some(depName => !isDeclared(manifestDeps, depName) && !Object.hasOwn(peerDependencies, depName))) {
    return false
  }

  return pEvery.default(Object.keys(manifestDeps), async (depName) => {
    if (!isDeclared(lockfileDeps, depName)) {
      return false
    }
    if (Object.hasOwn(peerDependencies, depName)) return true
    const currentSpec = resolveCatalogSpec(localDep, depName, manifestDeps[depName])
    if (currentSpec == null) return false
    return dependencyIsUpToDate(localDep, {
      depName,
      currentSpec,
      lockfileDep: lockfileDeps[depName],
    })
  })
}

function isDeclared (deps: Record<string, string>, depName: string): boolean {
  return Object.hasOwn(deps, depName) && Boolean(deps[depName])
}

/**
 * The spec with a `catalog:` reference replaced by the catalog's entry, or
 * `undefined` when the catalog has no entry for the dependency.
 */
function resolveCatalogSpec (localDep: LocalDirectoryDep, depName: string, spec: string): string | undefined {
  const result = resolveFromCatalog(localDep.catalogs ?? {}, { alias: depName, bareSpecifier: spec })
  switch (result.type) {
    case 'found': return result.resolution.specifier
    case 'misconfiguration': return undefined
    case 'unused': return spec
  }
}

async function dependencyIsUpToDate (
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
  if (currentSpec.startsWith('workspace:')) {
    return workspaceDependencyIsUpToDate(localDep, { depName, currentSpec, cleanLockfileDep, lockfilePath })
  }
  if (!hasExpectedName(cleanLockfileDep, { depName, currentSpec })) {
    return false
  }
  const lockfileVersion = getDepVersion(cleanLockfileDep)
  return semver.satisfies(lockfileVersion, getVersionRange(currentSpec), { loose: true })
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
