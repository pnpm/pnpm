import fs from 'fs'
import path from 'path'
import util from 'util'
import { type FetchFromDirOptions, fetchFromDir } from '@pnpm/directory-fetcher'
import { PnpmError } from '@pnpm/error'

export const DIR: unique symbol = Symbol('Path is a directory')

// symbols and numbers are used instead of discriminated union because
// it's faster and simpler to compare primitives than to deep compare objects
export type File = number // representing the file's inode, which is sufficient for hardlinks
export type Dir = typeof DIR

export type Value = File | Dir
export type InodeMap = Record<string, Value>

export interface DiffItemBase {
  path: string
  oldValue?: Value
  newValue?: Value
}

export interface AddedItem extends DiffItemBase {
  path: string
  oldValue?: undefined
  newValue: Value
}

export interface RemovedItem extends DiffItemBase {
  path: string
  oldValue: Value
  newValue?: undefined
}

export interface ModifiedItem extends DiffItemBase {
  path: string
  oldValue: Value
  newValue: Value
}

export interface DirDiff {
  added: AddedItem[]
  removed: RemovedItem[]
  modified: ModifiedItem[]
}

// length comparison should place every directory before the files it contains because
// a directory path is always shorter than any file path it contains
const comparePaths = (a: string, b: string): number => (a.split(/\\|\//).length - b.split(/\\|\//).length) || a.localeCompare(b)

/**
 * Get the difference between 2 files tree.
 *
 * The arrays in the resulting object are sorted in such a way that every directory paths are placed before
 * the files it contains. This way, it would allow optimization for operations upon this diff.
 * Note that when performing removal of removed files according to this diff, the `removed` array should be reversed first.
 */
export function diffDir (oldIndex: InodeMap, newIndex: InodeMap): DirDiff {
  const oldPaths = Object.keys(oldIndex).sort(comparePaths)
  const newPaths = Object.keys(newIndex).sort(comparePaths)

  const removed: RemovedItem[] = oldPaths
    .filter(path => !(path in newIndex))
    .map(path => ({ path, oldValue: oldIndex[path] }))

  const added: AddedItem[] = newPaths
    .filter(path => !(path in oldIndex))
    .map(path => ({ path, newValue: newIndex[path] }))

  const modified: ModifiedItem[] = oldPaths
    .filter(path => path in newIndex && oldIndex[path] !== newIndex[path])
    .map(path => ({ path, oldValue: oldIndex[path], newValue: newIndex[path] }))

  return { added, removed, modified }
}

/**
 * Apply a patch on a directory.
 *
 * The {@link optimizedDirPatch} is assumed to be already optimized (i.e. `removed` is already reversed).
 */
export async function applyPatch (optimizedDirPatch: DirDiff, sourceDir: string, targetDir: string): Promise<void> {
  async function addRecursive (sourcePath: string, targetPath: string, value: Value): Promise<void> {
    if (value === DIR) {
      await fs.promises.mkdir(targetPath, { recursive: true })
    } else if (typeof value === 'number') {
      fs.mkdirSync(path.dirname(targetPath), { recursive: true })
      await fs.promises.link(sourcePath, targetPath)
    } else {
      const _: never = value // static type guard
    }
  }

  async function removeRecursive (targetPath: string): Promise<void> {
    try {
      await fs.promises.rm(targetPath, { recursive: true, force: true })
    } catch (error) {
      if (!util.types.isNativeError(error) || !('code' in error) || (error.code !== 'ENOENT')) {
        throw error
      }
    }
  }

  const adding = Promise.all(optimizedDirPatch.added.map(async item => {
    const sourcePath = path.join(sourceDir, item.path)
    const targetPath = path.join(targetDir, item.path)
    await addRecursive(sourcePath, targetPath, item.newValue)
  }))

  const removing = Promise.all(optimizedDirPatch.removed.map(async item => {
    const targetPath = path.join(targetDir, item.path)
    await removeRecursive(targetPath)
  }))

  const modifying = Promise.all(optimizedDirPatch.modified.map(async item => {
    const sourcePath = path.join(sourceDir, item.path)
    const targetPath = path.join(targetDir, item.path)
    if (item.oldValue === item.newValue) return
    await removeRecursive(targetPath)
    await addRecursive(sourcePath, targetPath, item.newValue)
  }))

  await Promise.all([adding, removing, modifying])
}

export type ExtendFilesMapStats = Pick<fs.Stats, 'ino' | 'isFile' | 'isDirectory'>

export interface ExtendFilesMapOptions {
  /** Map relative path of each file to their real path */
  filesMap: Map<string, string>
  /** Map relative path of each file to their stats */
  filesStats?: Record<string, ExtendFilesMapStats | null>
}

/**
 * Convert a pair of a files index map, which is a map from relative path of each file to their real paths,
 * and an optional file stats map, which is a map from relative path of each file to their stats,
 * into an inodes map, which is a map from relative path of every file and directory to their inode type.
 */
export async function extendFilesMap ({ filesMap, filesStats }: ExtendFilesMapOptions): Promise<InodeMap> {
  const result: InodeMap = {
    '.': DIR,
  }

  function addInodeAndAncestors (relativePath: string, value: Value): void {
    if (relativePath && relativePath !== '.' && !result[relativePath]) {
      result[relativePath] = value
      addInodeAndAncestors(path.dirname(relativePath), DIR)
    }
  }

  await Promise.all(Array.from(filesMap.entries()).map(async ([relativePath, realPath]) => {
    const stats = filesStats?.[relativePath] ?? await fs.promises.stat(realPath)
    if (stats.isFile()) {
      addInodeAndAncestors(relativePath, stats.ino)
    } else if (stats.isDirectory()) {
      addInodeAndAncestors(relativePath, DIR)
    } else {
      throw new PnpmError('UNSUPPORTED_INODE_TYPE', `Filesystem inode at ${realPath} is neither a file, a directory, or a symbolic link`)
    }
  }))

  return result
}

export class DirPatcher {
  private readonly sourceDir: string
  private readonly targetDir: string
  private readonly patch: DirDiff

  private constructor (patch: DirDiff, sourceDir: string, targetDir: string) {
    this.patch = patch
    this.sourceDir = sourceDir
    this.targetDir = targetDir
  }

  static async fromMultipleTargets (sourceDir: string, targetDirs: string[]): Promise<DirPatcher[]> {
    const fetchOptions: FetchFromDirOptions = {
      resolveSymlinks: false,
    }

    async function loadMap (dir: string): Promise<[InodeMap, string]> {
      const fetchResult = await fetchFromDir(dir, fetchOptions)
      return [await extendFilesMap(fetchResult), dir]
    }

    const [[sourceMap], targetPairs] = await Promise.all([
      loadMap(sourceDir),
      Promise.all(targetDirs.map(loadMap)),
    ])

    return targetPairs.map(([targetMap, targetDir]) => {
      const diff = diffDir(targetMap, sourceMap)

      // Before reversal, every directory in `diff.removed` are placed before its files.
      // After reversal, every file is place before its ancestors,
      // leading to children being deleted before parents, optimizing performance.
      diff.removed.reverse()

      return new this(diff, sourceDir, targetDir)
    })
  }

  async apply (): Promise<void> {
    await applyPatch(this.patch, this.sourceDir, this.targetDir)
  }
}
