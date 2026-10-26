import fs from 'node:fs'
import path from 'node:path'

import { isError } from '@pnpm/error'
import { isSubdir } from 'is-subdir'

export interface ResolvedDependency {
  dir: string
  realDir: string
}

/**
 * The node_modules directory that holds depName when resolved from fromDir:
 * fromDir and its ancestors up to the boundary that contains it, then each
 * boundary fromDir lies outside of. A dependency whose real directory lies
 * outside every boundary is not bundled.
 */
export function resolveDependency (depName: string, fromDir: string, boundaries: string[]): ResolvedDependency | undefined {
  if (!isSafeBundleName(depName)) return undefined
  for (const dir of getDependencySearchDirs(fromDir, boundaries)) {
    const found = findInNodeModulesOf(dir, depName)
    if (found != null) return keepWithinBoundaries(found, boundaries)
  }
  return undefined
}

// Bundle names come from package.json, so reject paths before joining them under node_modules.
export function isSafeBundleName (name: unknown): name is string {
  if (typeof name !== 'string' || name.includes('\\')) return false
  const parts = name.split('/')
  if (parts.length > 2 || (parts.length === 2 && !parts[0].startsWith('@'))) return false
  return parts.every((part) => part !== '' && part !== '.' && part !== '..')
}

function *getDependencySearchDirs (fromDir: string, boundaries: string[]): Generator<string> {
  let dir = fromDir
  while (isWithinAnyBoundary(dir, boundaries)) {
    yield dir
    const parent = path.dirname(dir)
    if (parent === dir || !isWithinAnyBoundary(parent, boundaries)) break
    dir = parent
  }
  for (const boundary of boundaries) {
    if (!isWithinBoundary(fromDir, boundary)) yield boundary
  }
}

function isWithinAnyBoundary (dir: string, boundaries: string[]): boolean {
  return boundaries.some((boundary) => isWithinBoundary(dir, boundary))
}

function isWithinBoundary (dir: string, boundary: string): boolean {
  return dir === boundary || isSubdir(boundary, dir)
}

function keepWithinBoundaries (resolved: ResolvedDependency, boundaries: string[]): ResolvedDependency | undefined {
  return boundaries.some((boundary) => isWithinBoundary(resolved.realDir, boundary)) ? resolved : undefined
}

function findInNodeModulesOf (dir: string, depName: string): ResolvedDependency | undefined {
  if (path.basename(dir) === 'node_modules') return undefined
  const candidate = path.join(dir, 'node_modules', depName)
  const realDir = realpathOfPackageDir(candidate)
  return realDir == null ? undefined : { dir: candidate, realDir }
}

function realpathOfPackageDir (dir: string): string | undefined {
  try {
    if (!fs.statSync(path.join(dir, 'package.json')).isFile()) return undefined
    return fs.realpathSync(dir)
  } catch (err: unknown) {
    if (!isError(err) || !('code' in err) || err.code !== 'ENOENT') throw err
    return undefined
  }
}
