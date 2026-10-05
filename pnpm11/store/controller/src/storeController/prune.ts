import { type Dirent, promises as fs } from 'node:fs'
import path from 'node:path'

import { isError } from '@pnpm/error'
import { globalInfo, globalWarn } from '@pnpm/logger'
import type { PackageFilesIndex } from '@pnpm/store.cafs'
import type { StoreIndex } from '@pnpm/store.index'
import { rimraf } from '@zkochan/rimraf'
import prettyBytes from 'pretty-bytes'

import { pruneGlobalVirtualStore } from './pruneGlobalVirtualStore.js'

const BIG_ONE = BigInt(1) as unknown

export interface PruneOptions {
  cacheDir: string
  storeDir: string
  storeIndex: StoreIndex
}

export async function prune ({ cacheDir, storeDir, storeIndex }: PruneOptions, removeAlienFiles?: boolean): Promise<void> {
  // 1. First, prune the global virtual store
  // This must happen BEFORE pruning the CAS, because removing packages from
  // the virtual store will reduce hard link counts on files in the CAS
  await pruneGlobalVirtualStore(storeDir)

  // 2. Clean up metadata cache
  await removeCachedMetadata(cacheDir, storeDir)
  await removeStaleSpillDirectories(storeDir)

  // 3. Prune the content-addressable store (CAS)
  const removedHashes = await pruneContentAddressableStore(storeDir, removeAlienFiles)

  // 4. Clean up orphaned package index entries
  removeOrphanedIndexEntries(storeIndex, removedHashes)
}

async function removeCachedMetadata (cacheDir: string, storeDir: string): Promise<void> {
  // Metadata dirs may be at top level (legacy metadata-*) or under a version prefix (vN/metadata*)
  const metadataDirs = await getSubdirsSafely(cacheDir)
  await Promise.all(metadataDirs.map(async (metadataDir) => {
    if (!metadataDir.startsWith('metadata') && !/^v\d+$/.test(metadataDir)) return
    await rimrafIgnoringMissing(path.join(cacheDir, metadataDir))
  }))
  await rimraf(path.join(storeDir, 'tmp'))
  globalInfo('Removed all cached metadata files')
}

async function removeStaleSpillDirectories (storeDir: string): Promise<void> {
  const cutoff = Date.now() - 24 * 60 * 60_000
  const directories = await getSubdirsSafely(storeDir)
  for (const directory of directories) {
    if (!directory.startsWith('download-') && !directory.startsWith('tarball-')) continue
    // eslint-disable-next-line no-await-in-loop -- bound filesystem work across abandoned spill directories
    await removeSpillDirectoryIfStale(path.join(storeDir, directory), cutoff)
  }
}

async function removeSpillDirectoryIfStale (directoryPath: string, cutoff: number): Promise<void> {
  try {
    const stats = await fs.lstat(directoryPath)
    if (stats.isDirectory() && stats.mtimeMs < cutoff && await spillFilesAreStale(directoryPath, cutoff)) {
      await rimrafIgnoringMissing(directoryPath)
    }
  } catch (err: unknown) {
    if (!(isError(err) && 'code' in err && err.code === 'ENOENT')) throw err
  }
}

async function spillFilesAreStale (directoryPath: string, cutoff: number): Promise<boolean> {
  for await (const file of await fs.opendir(directoryPath)) {
    const stats = await fs.lstat(path.join(directoryPath, file.name))
    if (stats.mtimeMs >= cutoff) return false
  }
  return true
}

async function rimrafIgnoringMissing (dir: string): Promise<void> {
  try {
    await rimraf(dir)
  } catch (err: unknown) {
    if (!(isError(err) && 'code' in err && err.code === 'ENOENT')) {
      throw err
    }
  }
}

interface CafsPruneStats {
  fileCounter: number
  totalSize: number
  removedHashes: Set<string>
}

async function pruneContentAddressableStore (storeDir: string, removeAlienFiles: boolean | undefined): Promise<Set<string>> {
  const cafsDir = path.join(storeDir, 'files')
  const stats: CafsPruneStats = {
    fileCounter: 0,
    totalSize: 0,
    removedHashes: new Set<string>(),
  }
  const dirs = await getSubdirsSafely(cafsDir)
  await Promise.all(dirs.map(async (dir) => {
    const subdir = path.join(cafsDir, dir)
    await Promise.all((await fs.readdir(subdir)).map(async (fileName) => {
      await pruneCafsEntry({ dir, fileName, subdir, removeAlienFiles, stats })
    }))
  }))
  globalInfo(`Removed ${stats.fileCounter} file${stats.fileCounter === 1 ? '' : 's'} (${prettyBytes(stats.totalSize)})`)
  return stats.removedHashes
}

interface PruneCafsEntryOptions {
  dir: string
  fileName: string
  subdir: string
  removeAlienFiles: boolean | undefined
  stats: CafsPruneStats
}

async function pruneCafsEntry ({ dir, fileName, subdir, removeAlienFiles, stats }: PruneCafsEntryOptions): Promise<void> {
  const filePath = path.join(subdir, fileName)
  const stat = await fs.stat(filePath)
  if (stat.isDirectory()) {
    if (removeAlienFiles) {
      await rimraf(filePath)
      globalWarn(`An alien directory has been removed from the store: ${filePath}`)
      stats.fileCounter++
    } else {
      globalWarn(`An alien directory is present in the store: ${filePath}`)
    }
    return
  }
  if (stat.nlink === 1 || stat.nlink === BIG_ONE) {
    stats.totalSize += stat.size
    await fs.unlink(filePath)
    stats.fileCounter++
    // Store the hex digest, which matches the format stored in PackageFileInfo.digest
    // The file name in the store is the hex representation of the hash (with optional -exec suffix)
    stats.removedHashes.add(`${dir}${fileName.replace(/-exec$/, '')}`)
  }
}

function removeOrphanedIndexEntries (storeIndex: StoreIndex, removedHashes: Set<string>): void {
  let pkgCounter = 0
  const toDelete: string[] = []
  for (const [filesIndexFile, data] of storeIndex.entries()) {
    const pkgFilesIndex = data as PackageFilesIndex
    const pkgJson = pkgFilesIndex.files.get('package.json')
    // TODO: implement prune of Node.js packages, they don't have a package.json file
    if (pkgJson && removedHashes.has(pkgJson.digest)) {
      toDelete.push(filesIndexFile)
      pkgCounter++
    }
  }
  storeIndex.deleteMany(toDelete)
  globalInfo(`Removed ${pkgCounter} package${pkgCounter === 1 ? '' : 's'}`)
}

async function getSubdirsSafely (dir: string): Promise<string[]> {
  let entries: Dirent[]
  try {
    entries = await fs.readdir(dir, { withFileTypes: true }) as Dirent[]
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') {
      return []
    }
    throw err
  }
  return entries
    .filter(entry => entry.isDirectory())
    .map(dir => dir.name)
}
