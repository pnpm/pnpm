import path from 'node:path'
import { pathToFileURL } from 'node:url'

import { packageRootLinkTarget } from '@pnpm/deps.path'
import normalizePath from 'normalize-path'

export interface LinkTarget {
  id: string
  dir: string
}

/**
 * What a `link:` reference is relative to: the importer that declares it,
 * or, for a `link:<root>/...` reference, the directory of the package that
 * declares it.
 */
export interface LinkBase {
  importerId?: string
  packageDir?: string
}

export function resolveLinkTarget (lockfileDir: string, base: LinkBase | undefined, ref: string): LinkTarget {
  const packageRootTarget = packageRootLinkTarget(ref)
  const linkPath = packageRootTarget ?? ref.slice(5)
  // Detect the path flavor from `linkPath`, not the raw `ref`: the `link:`
  // prefix would hide a Windows-absolute target (e.g. `link:C:\x`) from the
  // drive-letter check, making it look relative on POSIX.
  const pathUtils = getPathUtils(lockfileDir, linkPath)
  const baseDir = packageRootTarget != null && base?.packageDir != null
    ? base.packageDir
    : pathUtils.resolve(lockfileDir, base?.importerId ?? '.')
  const dir = pathUtils.isAbsolute(linkPath)
    ? linkPath
    : pathUtils.resolve(baseDir, linkPath)
  const relativeId = relativePath(lockfileDir, dir)
  return {
    id: relativeId == null || relativeId.startsWith('..') ? `link:${normalizePath(dir)}` : relativeId,
    dir,
  }
}

export function toRelativeUrl (from: string, to: string): string {
  // No meaningful relative path exists between a POSIX dir and a
  // Windows-absolute target (or vice versa), so emit an absolute file URL
  // rather than letting `path.win32.relative` produce a bogus relative string.
  const toIsWindows = isWindowsAbsolutePath(to)
  if (toIsWindows !== isWindowsAbsolutePath(from)) {
    return pathToFileURL(to, { windows: toIsWindows }).href
  }
  const pathUtils = getPathUtils(from, to)
  const relative = pathUtils.relative(from, to)
  if (pathUtils.isAbsolute(relative)) {
    return pathToFileURL(to, { windows: pathUtils === path.win32 }).href
  }
  const normalizedRelativePath = normalizePath(relative) || '.'
  if (normalizedRelativePath === '.' || normalizedRelativePath === '..' || normalizedRelativePath.startsWith('./') || normalizedRelativePath.startsWith('../')) {
    return normalizedRelativePath
  }
  return `./${normalizedRelativePath}`
}

export function getNodeModulesPath (packageLocation: string): string | undefined {
  const segments = normalizePath(packageLocation).split('/')
  const nodeModulesIndex = segments.lastIndexOf('node_modules')
  if (nodeModulesIndex === -1) return undefined
  return segments.slice(0, nodeModulesIndex + 1).join('/')
}

type PathUtils = typeof path.posix

const WINDOWS_ABSOLUTE_PATH_REGEXP = /^(?:[a-z]:[\\/]|[/\\]{2}[^/\\])/i

export function resolvePath (from: string, ...segments: string[]): string {
  return getPathUtils(from, ...segments).resolve(from, ...segments)
}

export function joinPath (from: string, ...segments: string[]): string {
  return getPathUtils(from, ...segments).join(from, ...segments)
}

export function relativePath (from: string, to: string): string | undefined {
  const pathUtils = getPathUtils(from, to)
  const relative = pathUtils.relative(from, to)
  if (pathUtils.isAbsolute(relative)) return undefined
  return normalizePath(relative) || '.'
}

export function getPathUtils (...paths: string[]): PathUtils {
  return paths.some(isWindowsAbsolutePath) ? path.win32 : path
}

function isWindowsAbsolutePath (pathLike: string): boolean {
  return WINDOWS_ABSOLUTE_PATH_REGEXP.test(pathLike)
}

export function sortedEntries<Value> (entries: Iterable<[string, Value]>): Array<[string, Value]> {
  return Array.from(entries).sort(([a], [b]) => compareStrings(a, b))
}

function compareStrings (left: string, right: string): number {
  return left < right ? -1 : left > right ? 1 : 0
}
