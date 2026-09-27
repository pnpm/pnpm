import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import type { FilesMap, PackageFiles } from '@pnpm/store.cafs-types'

/**
 * The file-type bits of a side-effects entry that records a symlink. The
 * entry's content is the link target, stored in the CAFS as a regular file,
 * so a pnpm version that does not know this type can still verify it.
 */
export const SYMLINK_MODE = 0o120000

const FILE_TYPE_MASK = 0o170000

export function isSymlinkMode (mode: number): boolean {
  return (mode & FILE_TYPE_MASK) === SYMLINK_MODE
}

/**
 * The target the side-effects cache records for a symlink at `linkPath`, or
 * `undefined` when the link cannot be recorded.
 *
 * The link must be at a plain relative path, outside the top-level
 * `node_modules` and not at `package.json`. A recordable target is relative,
 * climbs no higher than the package root, climbs only at its start, and never
 * names a `node_modules` directory. The last rule keeps a restored link from
 * resolving into the dependencies that pnpm links next to the package, even
 * through another restored link. Reserved names match in any letter case,
 * as they do on a case-insensitive filesystem.
 */
export function normalizeSymlinkTarget (linkPath: string, target: string): string | undefined {
  const linkSegments = linkPath.split('/')
  if (
    !linkSegments.every(isPlainName) ||
    isName(linkSegments[0], 'node_modules') ||
    (linkSegments.length === 1 && isName(linkSegments[0], 'package.json'))
  ) {
    return undefined
  }
  if (target === '' || target.startsWith('/') || target.includes('\\') || target.includes('\0') || path.win32.parse(target).root !== '') {
    return undefined
  }
  const segments = target.split('/').filter((segment) => segment !== '' && segment !== '.')
  let parents = 0
  while (parents < segments.length && segments[parents] === '..') parents++
  const names = segments.slice(parents)
  if (names.length === 0 || names.some((name) => name === '..' || isName(name, 'node_modules'))) return undefined
  if (parents > linkSegments.length - 1) return undefined
  return segments.join('/')
}

function isPlainName (segment: string): boolean {
  return segment !== '' && segment !== '.' && segment !== '..' && !segment.includes('\\') && !segment.includes('\0')
}

function isName (segment: string, reservedName: string): boolean {
  return segment.toLowerCase() === reservedName
}

export interface SplitSymlinksResult {
  added: FilesMap
  symlinks: Map<string, string>
}

/**
 * Separates the symlink entries of one side-effects diff from its files and
 * reads their targets from the store.
 *
 * Returns `undefined` when the diff cannot be restored safely, in which case
 * the caller drops the whole cache entry and the package is built again. That
 * happens when a target is not in its recorded form, when the platform cannot
 * create the links, and when a file or link of the restored package would be
 * written below a link.
 */
export function splitSymlinks (
  diff: { added: PackageFiles, deleted?: string[] },
  addedMap: FilesMap,
  baseFiles: Iterable<string>
): SplitSymlinksResult | undefined {
  const symlinks = new Map<string, string>()
  const added: FilesMap = new Map()
  for (const [relativePath, filePath] of addedMap) {
    if (!isSymlinkMode(diff.added.get(relativePath)!.mode)) {
      added.set(relativePath, filePath)
      continue
    }
    const target = readSymlinkTarget(filePath)
    if (target == null || normalizeSymlinkTarget(relativePath, target) !== target) return undefined
    symlinks.set(relativePath, target)
  }
  if (symlinks.size === 0) return { added, symlinks }
  if (process.platform === 'win32') return undefined
  const deleted = new Set(diff.deleted)
  const paths = [...added.keys(), ...symlinks.keys()]
  for (const baseFile of baseFiles) {
    if (!deleted.has(baseFile)) paths.push(baseFile)
  }
  for (const relativePath of paths) {
    for (let slash = relativePath.lastIndexOf('/'); slash > 0; slash = relativePath.lastIndexOf('/', slash - 1)) {
      if (symlinks.has(relativePath.slice(0, slash))) return undefined
    }
  }
  return { added, symlinks }
}

function readSymlinkTarget (filePath: string): string | undefined {
  try {
    return fs.readFileSync(filePath, 'utf8')
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return undefined
    throw err
  }
}
