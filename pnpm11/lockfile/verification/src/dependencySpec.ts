import fs from 'node:fs'
import path from 'node:path'

import type { ProjectSnapshot } from '@pnpm/lockfile.types'
import { safeReadPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import { barePathIsUnambiguous, isTarballFilename } from '@pnpm/resolving.local-resolver'
import { findWorkspaceLinkTarget, type WorkspaceLinkContext } from '@pnpm/resolving.npm-resolver'
import type { DependencyManifest } from '@pnpm/types'
import { resolveWorkspaceRange } from '@pnpm/workspace.range-resolver'
import semver from 'semver'

export function isWorkspaceRangeSpec (spec: string): boolean {
  return spec.startsWith('workspace:') && !isWorkspacePath(spec.slice(10))
}

export function usesInjectionSetting (spec: string): boolean {
  return spec.startsWith('workspace:') || (barePathIsUnambiguous(spec) && !isTarballFilename(spec))
}

/**
 * Whether the resolver picks a workspace project at `version` for `range`. A
 * project without a version matches only the pick-the-highest tokens.
 */
export function versionSatisfies (version: string | undefined, range: string): boolean {
  return resolveWorkspaceRange(range, [version ?? '']) != null
}

export function getLocalPath (lockfileDep: string): string | null {
  if (lockfileDep.startsWith('link:') || lockfileDep.startsWith('file:')) {
    return lockfileDep.slice(5)
  }
  const aliasEnd = lockfileDep.indexOf('@file:', 1)
  return aliasEnd === -1 ? null : lockfileDep.slice(aliasEnd + 6)
}

export function getDepActualName (lockfileDep: string, defaultName: string): string {
  const atIndex = lockfileDep.lastIndexOf('@')
  if (atIndex > 0) {
    return lockfileDep.slice(0, atIndex)
  }
  return defaultName
}

export function getDepVersion (lockfileDep: string): string {
  const atIndex = lockfileDep.lastIndexOf('@')
  const ver = atIndex > 0 ? lockfileDep.slice(atIndex + 1) : lockfileDep
  const colonIndex = ver.indexOf(':')
  return colonIndex >= 0 ? ver.slice(colonIndex + 1) : ver
}

export function isSubdirectory (parentDir: string, childPath: string): boolean {
  const relativePath = path.relative(parentDir, childPath)
  return relativePath === '' || (
    relativePath !== '..' &&
    !relativePath.startsWith(`..${path.sep}`) &&
    !path.isAbsolute(relativePath)
  )
}

export function isWorkspacePath (spec: string): boolean {
  return (
    spec.startsWith('.') ||
    spec.startsWith('/') ||
    spec.startsWith('\\') ||
    spec.startsWith('~/') ||
    spec.startsWith('~\\') ||
    /^[a-z]:/i.test(spec)
  )
}

export function getTargetPkgName (spec: string, defaultName: string): string {
  if (spec.startsWith('workspace:')) {
    const raw = spec.slice(10)
    if (isWorkspacePath(raw)) return defaultName
    const atIndex = raw.lastIndexOf('@')
    if (atIndex > 0) {
      return raw.slice(0, atIndex)
    }
  } else if (spec.startsWith('npm:')) {
    const raw = spec.slice(4)
    if (semver.validRange(raw)) {
      return defaultName
    }
    const atIndex = raw.lastIndexOf('@')
    if (atIndex > 0) {
      return raw.slice(0, atIndex)
    }
    return raw
  }
  return defaultName
}

export function getVersionRange (spec: string): string {
  if (spec.startsWith('workspace:')) {
    const raw = spec.slice(10)
    const atIndex = raw.lastIndexOf('@')
    if (atIndex > 0) {
      return raw.slice(atIndex + 1) || '*'
    }
    return raw
  }
  if (spec.startsWith('npm:')) {
    const raw = spec.slice(4)
    if (semver.validRange(raw)) {
      return raw
    }
    const index = raw.indexOf('@', 1)
    if (index === -1) return '*'
    return raw.slice(index + 1) || '*'
  }
  return spec
}

/**
 * Whether `excludeLinksFromLockfile` leaves the dependency out of the
 * importer: a `link:` spec, or one the importer has no specifier for that a
 * package in `workspacePackages` matches.
 */
export function isExcludedLink (
  { workspacePackages, catalogs }: Partial<WorkspaceLinkContext>,
  { importer, depName, spec }: { importer: Pick<ProjectSnapshot, 'specifiers'>, depName: string, spec: string }
): boolean {
  if (spec.startsWith('link:')) return true
  return workspacePackages != null &&
    importer.specifiers[depName] == null &&
    findWorkspaceLinkTarget({ workspacePackages, catalogs }, { alias: depName, bareSpecifier: spec }) != null
}

export async function linkedWorkspaceTargetSatisfies (
  localDep: {
    lockfileDir: string
    manifestsByDir: Record<string, DependencyManifest>
    workspaceRoot: string
  },
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
  const targetPkg = await readTargetManifest(localDep, targetDir)
  const expectedName = getTargetPkgName(currentSpec, depName)
  if (!targetPkg || targetPkg.name !== expectedName) {
    return false
  }
  return versionSatisfies(targetPkg.version, range)
}

async function readTargetManifest (
  localDep: { manifestsByDir: Record<string, DependencyManifest>, workspaceRoot: string },
  targetDir: string
): Promise<DependencyManifest | null> {
  if (await realPathStaysInsideWorkspace(localDep.workspaceRoot, targetDir)) {
    return localDep.manifestsByDir[targetDir] ?? safeReadPackageJsonFromDir(targetDir)
  }
  return publishingProjectManifest(localDep, targetDir)
}

async function publishingProjectManifest (
  localDep: { manifestsByDir: Record<string, DependencyManifest>, workspaceRoot: string },
  targetDir: string
): Promise<DependencyManifest | null> {
  for (let candidate = path.dirname(targetDir); isSubdirectory(localDep.workspaceRoot, candidate); candidate = path.dirname(candidate)) {
    // eslint-disable-next-line no-await-in-loop -- climbs one ancestor at a time and stops at the first match
    const manifest = await projectPublishesFromDir(localDep, candidate, targetDir)
    if (manifest != null) return manifest
  }
  return null
}

async function projectPublishesFromDir (
  localDep: { manifestsByDir: Record<string, DependencyManifest>, workspaceRoot: string },
  candidate: string,
  targetDir: string
): Promise<DependencyManifest | null> {
  if (!await realPathStaysInsideWorkspace(localDep.workspaceRoot, candidate)) {
    return null
  }
  const manifest = localDep.manifestsByDir[candidate] ?? await safeReadPackageJsonFromDir(candidate)
  return manifest?.publishConfig?.directory != null && path.resolve(candidate, manifest.publishConfig.directory) === targetDir
    ? manifest
    : null
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
