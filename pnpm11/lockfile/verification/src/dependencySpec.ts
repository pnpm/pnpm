import os from 'node:os'
import path from 'node:path'

import semver from 'semver'

export function isWorkspaceRangeSpec (spec: string): boolean {
  return spec.startsWith('workspace:') && !isWorkspacePath(spec.slice(10))
}

export function versionSatisfies (version: string | undefined, range: string): boolean {
  // This should pass the same options to semver as @pnpm/resolving.npm-resolver
  return range === '*' || range === '^' || range === '~' ||
    (version != null && semver.satisfies(version, range, { loose: true }))
}

export function getLocalPath (lockfileDep: string): string | null {
  return lockfileDep.startsWith('link:') || lockfileDep.startsWith('file:')
    ? lockfileDep.slice(5)
    : null
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

export function resolveSpecPath (baseDir: string, rawPath: string): string {
  const clean = rawPath.startsWith('./') ? rawPath.slice(2) : rawPath
  if (clean.startsWith('~/') || clean.startsWith('~\\')) {
    return path.resolve(os.homedir(), clean.slice(2))
  }
  return path.resolve(baseDir, clean)
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
