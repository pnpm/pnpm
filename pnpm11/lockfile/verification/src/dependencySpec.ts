import fs from 'node:fs'
import path from 'node:path'

import { isError } from '@pnpm/error'
import type { ProjectSnapshot } from '@pnpm/lockfile.types'
import { safeReadPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import { barePathIsUnambiguous, isTarballFilename } from '@pnpm/resolving.local-resolver'
import { findWorkspaceLinkTarget, parseBareSpecifier, type WorkspaceLinkContext } from '@pnpm/resolving.npm-resolver'
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
  // A package name has no colon. A URL that contains `@file:` does.
  if (aliasEnd === -1 || lockfileDep.slice(0, aliasEnd).includes(':')) return null
  return lockfileDep.slice(aliasEnd + 6)
}

/**
 * The range the resolver picks a workspace version with for a plain or `npm:`
 * spec. A tag picks the highest version. `null` for a spec that names no
 * version, such as a URL.
 */
export function registrySpecWorkspaceRange (spec: string, alias: string): string | null {
  const parsed = parseBareSpecifier(spec, alias, 'latest', '')
  if (parsed == null) return null
  return parsed.type === 'tag' ? '*' : parsed.fetchSpec
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
  localDep: LinkedWorkspaceDep,
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

type ManifestSource = Pick<LinkedWorkspaceDep, 'manifestsByDir' | 'workspaceRoot'>

interface LinkedWorkspaceDep {
  lockfileDir: string
  manifestsByDir: Record<string, DependencyManifest>
  workspaceRoot: string
}

/**
 * The manifest that describes a recorded workspace target: a loaded project
 * at the target, else the project publishing from it, else the target's own
 * `package.json`. A target or manifest that resolves outside the workspace
 * has none.
 */
async function readTargetManifest (localDep: ManifestSource, targetDir: string): Promise<DependencyManifest | null> {
  const realWorkspaceRoot = await realpathOrNull(localDep.workspaceRoot)
  if (realWorkspaceRoot == null) return null
  if (localDep.manifestsByDir[targetDir] == null) {
    const publishing = await publishingProjectManifest(localDep, realWorkspaceRoot, targetDir)
    if (publishing != null) return publishing
  }
  return readContainedManifest(localDep, realWorkspaceRoot, targetDir)
}

/**
 * The manifest of the project whose `publishConfig.directory` the resolver
 * links to `targetDir`. A directory not built yet is described by that project.
 */
async function publishingProjectManifest (
  localDep: ManifestSource,
  realWorkspaceRoot: string,
  targetDir: string
): Promise<DependencyManifest | null> {
  if (await entryExists(targetDir) && !await realPathStaysInside(realWorkspaceRoot, targetDir)) {
    return null
  }
  let candidate = path.dirname(targetDir)
  while (isSubdirectory(localDep.workspaceRoot, candidate)) {
    // eslint-disable-next-line no-await-in-loop -- climbs one ancestor at a time and stops at the first match
    const manifest = await readContainedManifest(localDep, realWorkspaceRoot, candidate)
    if (linksPublishDirectory(manifest, candidate, targetDir)) return manifest
    const parent = path.dirname(candidate)
    if (parent === candidate) return null
    candidate = parent
  }
  return null
}

function linksPublishDirectory (
  manifest: DependencyManifest | null,
  projectDir: string,
  targetDir: string
): manifest is DependencyManifest {
  const publishConfig = manifest?.publishConfig
  return publishConfig?.directory != null &&
    publishConfig.linkDirectory !== false &&
    path.resolve(projectDir, publishConfig.directory) === targetDir
}

/**
 * The manifest of `dir`, the loaded one first, when `dir` and any
 * `package.json` in it resolve inside the workspace. A loaded project may
 * have no `package.json`.
 */
async function readContainedManifest (
  localDep: ManifestSource,
  realWorkspaceRoot: string,
  dir: string
): Promise<DependencyManifest | null> {
  if (!await realPathStaysInside(realWorkspaceRoot, dir)) return null
  const loaded = localDep.manifestsByDir[dir]
  const realManifestPath = await realpathOrNull(path.join(dir, 'package.json'))
  if (realManifestPath == null) return loaded ?? null
  if (!isSubdirectory(realWorkspaceRoot, realManifestPath)) return null
  return loaded ?? safeReadPackageJsonFromDir(dir)
}

async function realPathStaysInside (realRoot: string, target: string): Promise<boolean> {
  const realTarget = await realpathOrNull(target)
  return realTarget != null && isSubdirectory(realRoot, realTarget)
}

async function realpathOrNull (target: string): Promise<string | null> {
  try {
    return await fs.promises.realpath(target)
  } catch {
    return null
  }
}

/** A dangling symlink counts as present, unlike in `path-exists`. */
async function entryExists (target: string): Promise<boolean> {
  try {
    await fs.promises.lstat(target)
    return true
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && (err.code === 'ENOENT' || err.code === 'ENOTDIR')) return false
    throw err
  }
}
