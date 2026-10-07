import fs from 'node:fs'
import path from 'node:path'
import { pipeline } from 'node:stream/promises'

import { isError } from '@pnpm/error'
import { fetchFromDir, type FetchFromDirOptions } from '@pnpm/fetching.directory-fetcher'
import { renameFileWithRetry, renameFileWithRetryAsync } from '@pnpm/fs.graceful-fs'
import { pathTemp } from 'path-temp'

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
const comparePaths = (left: string, right: string): number => (left.split(/\\|\//).length - right.split(/\\|\//).length) || left.localeCompare(right)

/**
 * Get the difference between 2 files tree.
 *
 * The arrays in the resulting object are sorted in such a way that every directory paths are placed before
 * the files it contains.
 * Note that when performing removal of removed files according to this diff, the `removed` array should be reversed first.
 */
export function diffDir (oldIndex: InodeMap, newIndex: InodeMap): DirDiff {
  const oldPaths = Object.keys(oldIndex).sort(comparePaths)
  const newPaths = Object.keys(newIndex).sort(comparePaths)

  const removed: RemovedItem[] = oldPaths
    .filter(path => !Object.hasOwn(newIndex, path))
    .map(path => ({ path, oldValue: oldIndex[path] }))

  const added: AddedItem[] = newPaths
    .filter(path => !Object.hasOwn(oldIndex, path))
    .map(path => ({ path, newValue: newIndex[path] }))

  const modified: ModifiedItem[] = oldPaths
    .filter(path => Object.hasOwn(newIndex, path) && oldIndex[path] !== newIndex[path])
    .map(path => ({ path, oldValue: oldIndex[path], newValue: newIndex[path] }))

  return { added, removed, modified }
}

/**
 * Apply a patch on a directory.
 *
 * The {@link optimizedDirPatch} is assumed to be already optimized (i.e. `removed` is already reversed).
 */
export async function applyPatch (optimizedDirPatch: DirDiff, sourceDir: string, targetDir: string): Promise<void> {
  const changes = collectPatchChanges(optimizedDirPatch)
  const newDirs = changes.filter(item => item.newValue === DIR).sort((a, b) => comparePaths(a.path, b.path))
  const newFiles = changes.filter(item => item.newValue !== DIR)

  for (const item of optimizedDirPatch.removed) {
    await removeRecursive(path.join(targetDir, item.path)) // eslint-disable-line no-await-in-loop -- removals finish before the directory pass
  }

  for (const item of newDirs) {
    await applyPatchChange(item, sourceDir, targetDir) // eslint-disable-line no-await-in-loop -- a directory is created only after its parent
  }
  await Promise.all(newFiles.map(item => applyPatchChange(item, sourceDir, targetDir)))
}

function collectPatchChanges (optimizedDirPatch: DirDiff): Array<AddedItem | ModifiedItem> {
  return [...optimizedDirPatch.added, ...optimizedDirPatch.modified]
    .filter(item => item.oldValue !== item.newValue)
}

async function applyPatchChange (item: AddedItem | ModifiedItem, sourceDir: string, targetDir: string): Promise<void> {
  const sourcePath = path.join(sourceDir, item.path)
  const targetPath = path.join(targetDir, item.path)
  if (item.oldValue !== undefined) {
    await removeRecursive(targetPath)
  }
  await addRecursive(sourcePath, targetPath, item.newValue)
}

async function addRecursive (sourcePath: string, targetPath: string, value: Value): Promise<void> {
  if (value === DIR) {
    await retryOverBlockingInode(targetPath, async () => fs.promises.mkdir(targetPath, { recursive: true }))
  } else if (typeof value === 'string') {
    fs.mkdirSync(path.dirname(targetPath), { recursive: true })
    await retryOverBlockingInode(targetPath, async () => linkOrCopy(sourcePath, targetPath))
  }
}

async function linkOrCopy (sourcePath: string, targetPath: string): Promise<void> {
  try {
    await fs.promises.link(sourcePath, targetPath)
  } catch (error) {
    if (isError(error) && 'code' in error && error.code === 'EXDEV') {
      await copyIntoPlace(sourcePath, targetPath)
      return
    }
    throw error
  }
}

async function copyIntoPlace (sourcePath: string, targetPath: string): Promise<void> {
  const tempPath = pathTemp(path.dirname(targetPath))
  try {
    await fs.promises.copyFile(sourcePath, tempPath, fs.constants.COPYFILE_EXCL)
    renameFileWithRetry(tempPath, targetPath)
  } catch (error) {
    await fs.promises.rm(tempPath, { force: true })
    throw error
  }
}

async function retryOverBlockingInode (targetPath: string, add: () => Promise<unknown>): Promise<void> {
  try {
    await add()
  } catch (error) {
    if (!isError(error) || !('code' in error) || (error.code !== 'EEXIST')) {
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
    if (!isError(error) || !('code' in error) || (error.code !== 'ENOENT')) {
      throw error
    }
  }
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
    if (relativePath && relativePath !== '.' && !Object.hasOwn(result, relativePath)) {
      // A plain assignment would set the prototype for a file named `__proto__`.
      Object.defineProperty(result, relativePath, { value, writable: true, enumerable: true, configurable: true })
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

const WATCH_MTIME_TOLERANCE_MS = 1

const WATCH_FETCH_OPTIONS: FetchFromDirOptions = { resolveSymlinks: false }

/** A source directory's files, read once per poll and published to each of its injected copies. */
export interface PublishSource {
  dir: string
  map: InodeMap
}

export async function readPublishSource (dir: string): Promise<PublishSource> {
  return { dir, map: await extendFilesMap(await fetchFromDir(dir, WATCH_FETCH_OPTIONS)) }
}

/**
 * Copy changed files into `targetDir` as independent files, so a watcher on
 * the injected directory sees the write. A hardlink edited in place is
 * republished when its mtime is at least `editedSinceMs`. A copy whose size
 * and mtime already match the source is left alone.
 */
export async function publishEditsForWatchers (
  source: PublishSource,
  targetDir: string,
  editedSinceMs: number
): Promise<void> {
  const { dir: sourceDir, map: sourceMap } = source
  const targetMap = await extendFilesMap(await fetchFromDir(targetDir, WATCH_FETCH_OPTIONS))

  await syncRemovedWatchPaths(targetMap, sourceMap, targetDir)
  const sourcePaths = Object.keys(sourceMap).sort(comparePaths)
  await syncDirectoryWatchPaths(sourcePaths, sourceMap, targetMap, targetDir)
  await syncFileWatchPaths({ sourcePaths, sourceMap, targetMap, sourceDir, targetDir, editedSinceMs })
}

async function syncRemovedWatchPaths (targetMap: InodeMap, sourceMap: InodeMap, targetDir: string): Promise<void> {
  const removed = Object.keys(targetMap)
    .filter(relPath => !Object.hasOwn(sourceMap, relPath) && relPath !== '.')
    .sort(comparePaths)
    .reverse()
  for (const relPath of removed) {
    await removePath(path.join(targetDir, relPath)) // eslint-disable-line no-await-in-loop -- children are removed before parent
  }
}

async function syncDirectoryWatchPaths (
  sourcePaths: string[],
  sourceMap: InodeMap,
  targetMap: InodeMap,
  targetDir: string
): Promise<void> {
  for (const relPath of sourcePaths) {
    if (sourceMap[relPath] !== DIR || relPath === '.') continue
    const targetPath = path.join(targetDir, relPath)
    if (Object.hasOwn(targetMap, relPath) && targetMap[relPath] !== DIR) {
      await removePath(targetPath) // eslint-disable-line no-await-in-loop -- directory created after parent
    }
    await fs.promises.mkdir(targetPath, { recursive: true }) // eslint-disable-line no-await-in-loop -- directory created after parent
  }
}

async function syncFileWatchPaths ({
  sourcePaths,
  sourceMap,
  targetMap,
  sourceDir,
  targetDir,
  editedSinceMs,
}: {
  sourcePaths: string[]
  sourceMap: InodeMap
  targetMap: InodeMap
  sourceDir: string
  targetDir: string
  editedSinceMs: number
}): Promise<void> {
  for (const relPath of sourcePaths) {
    const sourceValue = sourceMap[relPath]
    if (typeof sourceValue !== 'string') continue
    const sourcePath = path.join(sourceDir, relPath)
    const targetPath = path.join(targetDir, relPath)
    const sourceStat = await fs.promises.stat(sourcePath) // eslint-disable-line no-await-in-loop -- published in path order
    const targetValue = targetMap[relPath]
    const targetStat = typeof targetValue === 'string'
      ? await statFile(targetPath) // eslint-disable-line no-await-in-loop -- published in path order
      : null
    if (!shouldPublish({ sourceStat, targetStat, sourceId: sourceValue, targetValue, editedSinceMs })) continue
    await copyForWatchers(sourcePath, targetPath, sourceStat) // eslint-disable-line no-await-in-loop -- published in path order
  }
}

function shouldPublish ({ sourceStat, targetStat, sourceId, targetValue, editedSinceMs }: {
  sourceStat: fs.Stats
  targetStat: fs.Stats | null
  sourceId: string
  targetValue: Value | undefined
  editedSinceMs: number
}): boolean {
  if (targetStat == null || typeof targetValue !== 'string') return true
  if (targetValue === sourceId) return sourceStat.mtimeMs >= editedSinceMs
  return sourceStat.size !== targetStat.size ||
    Math.abs(sourceStat.mtimeMs - targetStat.mtimeMs) > WATCH_MTIME_TOLERANCE_MS
}

async function statFile (filePath: string): Promise<fs.Stats | null> {
  try {
    return await fs.promises.stat(filePath)
  } catch (error: unknown) {
    if (isEnoent(error)) return null
    throw error
  }
}

function isEnoent (error: unknown): boolean {
  return isError(error) && 'code' in error && error.code === 'ENOENT'
}

async function removePath (targetPath: string): Promise<void> {
  await fs.promises.rm(targetPath, { recursive: true, force: true })
}

/**
 * Read and write the bytes. `fs.copyFile` may reflink on macOS, and a
 * reflink does not notify watchers the way a new file in the injected
 * directory does.
 */
async function copyForWatchers (
  sourcePath: string,
  targetPath: string,
  sourceStat: Pick<fs.Stats, 'mode' | 'mtime'>
): Promise<void> {
  const existing = await fs.promises.lstat(targetPath).catch((error: unknown) => {
    if (isEnoent(error)) return null
    throw error
  })
  if (existing?.isDirectory() === true) {
    await removePath(targetPath)
  }
  await fs.promises.mkdir(path.dirname(targetPath), { recursive: true })
  const tempPath = pathTemp(path.dirname(targetPath))
  try {
    await pipeline(fs.createReadStream(sourcePath), fs.createWriteStream(tempPath, { flags: 'wx' }))
    await fs.promises.chmod(tempPath, sourceStat.mode & 0o7777)
    await renameFileWithRetryAsync(tempPath, targetPath)
  } catch (error) {
    await fs.promises.rm(tempPath, { force: true })
    throw error
  }
  // Best effort: a target left with a different mtime is only published again on the next poll.
  await fs.promises.utimes(targetPath, new Date(), sourceStat.mtime).catch(() => {})
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

  static async fromMultipleTargets (sourceDir: string, targetDirs: string[], includeOnlyPackageFiles?: boolean): Promise<DirPatcher[]> {
    const fetchOptions: FetchFromDirOptions = {
      resolveSymlinks: false,
    }

    async function loadMap (dir: string, dirFetchOptions: FetchFromDirOptions): Promise<[InodeMap, string]> {
      const fetchResult = await fetchFromDir(dir, dirFetchOptions)
      return [await extendFilesMap(fetchResult), dir]
    }

    const [[sourceMap], targetPairs] = await Promise.all([
      loadMap(sourceDir, { ...fetchOptions, includeOnlyPackageFiles }),
      Promise.all(targetDirs.map(async (targetDir) => loadMap(targetDir, fetchOptions))),
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
