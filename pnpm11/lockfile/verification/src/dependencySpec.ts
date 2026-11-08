import path from 'node:path'

import type { ProjectSnapshot } from '@pnpm/lockfile.types'
import { findWorkspaceLinkTarget, type WorkspaceLinkContext } from '@pnpm/resolving.npm-resolver'
import { resolveWorkspaceRange } from '@pnpm/workspace.range-resolver'
import semver from 'semver'

export function isWorkspaceRangeSpec (spec: string): boolean {
  return spec.startsWith('workspace:') && !isWorkspacePath(spec.slice(10))
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
