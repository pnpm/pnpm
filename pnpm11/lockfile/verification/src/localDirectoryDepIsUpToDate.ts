import fs from 'node:fs'
import path from 'node:path'

import { removeSuffix } from '@pnpm/deps.path'
import type { PackageSnapshot } from '@pnpm/lockfile.types'
import { safeReadPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import type { DirectoryResolution } from '@pnpm/resolving.resolver-base'
import {
  DEPENDENCIES_OR_PEER_FIELDS,
  type DependencyManifest,
  type PeerDependenciesMeta,
} from '@pnpm/types'
import pEvery from 'p-every'
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
  manifestsByDir: Record<string, DependencyManifest>
  workspaceDir?: string
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
 * lockfile snapshot.
 */
export async function localDirectoryDepIsUpToDate (
  ctx: LocalDirectoryDepContext,
  pkgSnapshot: PackageSnapshot
): Promise<boolean> {
  if (!('directory' in (pkgSnapshot.resolution ?? {}))) return true
  const localDepDir = path.join(ctx.lockfileDir, (pkgSnapshot.resolution as DirectoryResolution).directory)
  const manifest = ctx.manifestsByDir[localDepDir] ?? await safeReadPackageJsonFromDir(localDepDir)
  if (!manifest) return false
  if (!peerDependenciesMetaMatch(manifest.peerDependenciesMeta ?? {}, pkgSnapshot.peerDependenciesMeta ?? {})) {
    return false
  }
  const localDep: LocalDirectoryDep = {
    lockfileDir: ctx.lockfileDir,
    localDepDir,
    manifestsByDir: ctx.manifestsByDir,
    workspaceRoot: ctx.workspaceDir ?? ctx.lockfileDir,
  }
  return pEvery.default(DEPENDENCIES_OR_PEER_FIELDS, async (depField) => {
    if (depField === 'devDependencies') return true
    return depFieldIsUpToDate(localDep, manifest[depField] ?? {}, pkgSnapshot[depField] ?? {})
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

async function depFieldIsUpToDate (
  localDep: LocalDirectoryDep,
  manifestDeps: Record<string, string>,
  lockfileDeps: Record<string, string>
): Promise<boolean> {
  if (Object.keys(lockfileDeps).some(depName => !Object.hasOwn(manifestDeps, depName) || !manifestDeps[depName])) {
    return false
  }

  return pEvery.default(Object.keys(manifestDeps), async (depName) => {
    if (!Object.hasOwn(lockfileDeps, depName) || !lockfileDeps[depName]) {
      return false
    }
    return dependencyIsUpToDate(localDep, {
      depName,
      currentSpec: manifestDeps[depName],
      lockfileDep: lockfileDeps[depName],
    })
  })
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
