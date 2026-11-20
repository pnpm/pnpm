import fs from 'node:fs'
import path from 'node:path'

import { isError } from '@pnpm/error'
import type { FilesMap, SideEffectsFilesMap } from '@pnpm/store.cafs-types'

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
  if (!isRecordableLinkPath(linkSegments) || !hasRecordableTargetForm(target)) {
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

function isRecordableLinkPath (linkSegments: string[]): boolean {
  return linkSegments.every(isPlainName) &&
    !isName(linkSegments[0], 'node_modules') &&
    !(linkSegments.length === 1 && isName(linkSegments[0], 'package.json'))
}

function hasRecordableTargetForm (target: string): boolean {
  return target !== '' && !target.startsWith('/') && !target.includes('\\') && !target.includes('\0') && path.win32.parse(target).root === ''
}

function isPlainName (segment: string): boolean {
  return segment !== '' && segment !== '.' && segment !== '..' && !segment.includes('\\') && !segment.includes('\0')
}

function isName (segment: string, reservedName: string): boolean {
  return segment.toLowerCase() === reservedName
}

/**
 * Collects one side-effects diff's added entries, resolved to store paths,
 * into the map the importer applies. A caller feeds it from the loop it
 * already runs over the diff, so a recorded symlink is told apart from a
 * file in that same pass and its target is read from the store as it goes.
 */
export interface SideEffectsFilesMapBuilder {
  add: (relativePath: string, mode: number, storePath: string) => void
  /**
   * The diff over the package's `baseFiles`, or `undefined` when it cannot
   * be restored safely, in which case the caller drops the whole cache entry
   * and the package is built again. That happens when a link's target is not
   * in its recorded form, when the platform cannot create the links, and when
   * a file or link of the restored package would be written below a link.
   */
  finish: (deleted: string[] | undefined, baseFiles: Iterable<string>) => SideEffectsFilesMap | undefined
}

export function createSideEffectsFilesMapBuilder (): SideEffectsFilesMapBuilder {
  const addedFiles: FilesMap = new Map()
  const addedSymlinks = new Map<string, string>()
  let restorable = true
  return { add, finish }

  function add (relativePath: string, mode: number, storePath: string): void {
    if (!isSymlinkMode(mode)) {
      addedFiles.set(relativePath, storePath)
      return
    }
    const target = restorable ? readRecordedSymlinkTarget(relativePath, storePath) : undefined
    if (target == null) {
      restorable = false
      return
    }
    addedSymlinks.set(relativePath, target)
  }

  function finish (deleted: string[] | undefined, baseFiles: Iterable<string>): SideEffectsFilesMap | undefined {
    if (!restorable) return undefined
    const filesMap: SideEffectsFilesMap = { added: addedFiles, deleted }
    if (addedSymlinks.size === 0) return filesMap
    if (process.platform === 'win32') return undefined
    const deletedSet = new Set(deleted)
    const restored = [...addedFiles.keys(), ...addedSymlinks.keys()]
    for (const baseFile of baseFiles) {
      if (!deletedSet.has(baseFile)) restored.push(baseFile)
    }
    if (writesBelowASymlink(addedSymlinks, restored)) return undefined
    filesMap.symlinks = addedSymlinks
    return filesMap
  }
}

/** Whether any of the `restored` paths sits below one of the `symlinks`. */
function writesBelowASymlink (symlinks: Map<string, string>, restored: string[]): boolean {
  return restored.some((relativePath) => {
    for (let slash = relativePath.lastIndexOf('/'); slash > 0; slash = relativePath.lastIndexOf('/', slash - 1)) {
      if (symlinks.has(relativePath.slice(0, slash))) return true
    }
    return false
  })
}

/**
 * The target of the symlink recorded at `linkPath`, read from its store file,
 * or `undefined` when the file is missing or does not hold a target in its
 * recorded form.
 */
function readRecordedSymlinkTarget (linkPath: string, filePath: string): string | undefined {
  let target: string
  try {
    target = fs.readFileSync(filePath, 'utf8')
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') return undefined
    throw err
  }
  return normalizeSymlinkTarget(linkPath, target) === target ? target : undefined
}
