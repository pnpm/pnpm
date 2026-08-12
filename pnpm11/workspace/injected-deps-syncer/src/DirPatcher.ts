import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { fetchFromDir, type FetchFromDirOptions } from '@pnpm/fetching.directory-fetcher'

export const DIR: unique symbol = Symbol('Path is a directory')

// symbols and strings are used instead of discriminated union because
// it's faster and simpler to compare primitives than to deep compare objects
/**
 * A file's identity, as `<device>:<inode>`. An inode number is only unique
 * within one filesystem, so the device it came from is part of the identity:
 * without it two unrelated files on different devices can collide and be
 * taken for the same file, leaving the injected copy stale.
 */
export type File = string
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
      await retryOverBlockingInode(targetPath, async () => fs.promises.mkdir(targetPath, { recursive: true }))
    } else if (typeof value === 'string') {
      fs.mkdirSync(path.dirname(targetPath), { recursive: true })
      await retryOverBlockingInode(targetPath, async () => fs.promises.link(sourcePath, targetPath))
    } else {
      const _: never = value // static type guard
    }
  }

  /**
   * The target may hold an inode that {@link extendFilesMap} skips — a FIFO, a
   * socket, a device. The diff cannot see it, so it is never scheduled for
   * removal, and adding over it fails with `EEXIST`. Clear that path and retry
   * once instead of aborting the sync partway through.
   */
  async function retryOverBlockingInode (targetPath: string, add: () => Promise<unknown>): Promise<void> {
    try {
      await add()
    } catch (error) {
      if (!util.types.isNativeError(error) || !('code' in error) || (error.code !== 'EEXIST')) {
        throw error
      }
      await removeRecursive(targetPath)
      await add()
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

  async function applyChange (item: AddedItem | ModifiedItem): Promise<void> {
    const sourcePath = path.join(sourceDir, item.path)
    const targetPath = path.join(targetDir, item.path)
    if (item.oldValue !== undefined) {
      await removeRecursive(targetPath)
    }
    await addRecursive(sourcePath, targetPath, item.newValue)
  }

  const changes: Array<AddedItem | ModifiedItem> = [...optimizedDirPatch.added, ...optimizedDirPatch.modified]
    .filter(item => item.oldValue !== item.newValue)
  const newDirs = changes.filter(item => item.newValue === DIR).sort((a, b) => comparePaths(a.path, b.path))
  const newFiles = changes.filter(item => item.newValue !== DIR)

  // The phase order is load-bearing twice over. Removals go first, so a path
  // the source turned from a directory into a file still has a directory in it
  // when its dropped children are unlinked. Directories then go in ahead of the
  // files they hold, so a directory is always empty when it displaces what the
  // target held at its path — otherwise a removal landing late would take out
  // files a sibling had already linked. A path the target holds as a file and
  // the source as a directory lands in `modified` rather than `added`, so both
  // arrays feed the directory pass.
  await Promise.all(optimizedDirPatch.removed.map(async item => {
    await removeRecursive(path.join(targetDir, item.path))
  }))

  for (const item of newDirs) {
    await applyChange(item) // eslint-disable-line no-await-in-loop
  }
  await Promise.all(newFiles.map(applyChange))
}

export type ExtendFilesMapStats = Pick<fs.Stats, 'dev' | 'ino' | 'isFile' | 'isDirectory'>

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
      addInodeAndAncestors(relativePath, fileId(stats))
    } else if (stats.isDirectory()) {
      addInodeAndAncestors(relativePath, DIR)
    }
    // Anything else — a FIFO, a socket, a device — cannot be hardlinked into
    // the injected copy, so it is left out of the map.
  }))

  return result
}

const fileId = (stats: Pick<ExtendFilesMapStats, 'dev' | 'ino'>): File => `${stats.dev}:${stats.ino}`

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
