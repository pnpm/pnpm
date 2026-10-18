import fs from 'node:fs'
import path from 'node:path'

import gfs, { renameFileWithRetry } from '@pnpm/fs.graceful-fs'
import { globalInfo } from '@pnpm/logger'
import { fastPathTemp as pathTemp } from 'path-temp'

import { clearDirBlockingFile, clearDirentBlockingDir, makeFileMapDirs } from './dirUtils.js'
import { fileModeForImport, isLinkableStoreMode, storeEntryModeForSource } from './fileMode.js'
import { filesHaveEqualContents } from './filesHaveEqualContents.js'
import { filesHaveSameIdentity } from './hardlinkedDirMatches.js'
import type { ImportFile, IndexedDirImport } from './importIndexedDir.js'

export function repairIndexedDir ({ importer, newDir, filenames, opts }: IndexedDirImport): void {
  makeFileMapDirs(newDir, filenames, { clearBlockers: true })
  let packageJsonSrc: string | undefined
  for (const [relativePath, src] of filenames) {
    if (relativePath === 'package.json') {
      packageJsonSrc = src
      continue
    }
    replaceFileIfDifferent(importer.importFile, src, path.join(newDir, relativePath))
  }
  for (const [relativePath, target] of opts.symlinks ?? []) {
    const dir = path.posix.dirname(relativePath)
    if (dir !== '.') clearDirentBlockingDir(newDir, dir)
    replaceSymlinkIfDifferent(target, path.join(newDir, relativePath))
  }
  if (packageJsonSrc !== undefined) {
    replaceFileIfDifferent(importer.importFile, packageJsonSrc, path.join(newDir, 'package.json'))
  }
}

export function replaceFileIfDifferent (importFile: ImportFile, src: string, dest: string): void {
  if (mismatchReason(dest, src) === undefined) return
  const tmp = pathTemp(dest)
  try {
    importFile(src, tmp)
  } catch (err) {
    try {
      fs.unlinkSync(tmp)
    } catch {}
    throw err
  }
  try {
    clearDirBlockingFile(dest)
    renameFileWithRetry(tmp, dest)
  } catch (err) {
    try {
      fs.unlinkSync(tmp)
    } catch {}
    if (mismatchReason(dest, src) === undefined) return
    throw err
  }
}

export function replaceSymlinkIfDifferent (target: string, dest: string): void {
  if (symlinkMatches(dest, target)) return
  fs.mkdirSync(path.dirname(dest), { recursive: true })
  const tmp = pathTemp(dest)
  fs.symlinkSync(target, tmp)
  try {
    clearDirBlockingFile(dest)
    renameFileWithRetry(tmp, dest)
  } catch (err) {
    try {
      fs.unlinkSync(tmp)
    } catch {}
    if (symlinkMatches(dest, target)) return
    throw err
  }
}

export function allFilesMatch (dir: string, filenames: Map<string, string>): boolean {
  const markerSrc = filenames.get('package.json')
  if (markerSrc !== undefined && !fileMatches(dir, 'package.json', markerSrc)) return false
  for (const [relativePath, src] of filenames) {
    if (relativePath === 'package.json') continue
    if (!fileMatches(dir, relativePath, src)) return false
  }
  return true
}

export function allSymlinksMatch (dir: string, symlinks: Map<string, string> | undefined): boolean {
  for (const [relativePath, target] of symlinks ?? []) {
    if (!symlinkMatches(path.join(dir, relativePath), target)) {
      globalInfo(`Re-importing "${dir}" because symlink "${relativePath}" does not point to "${target}"`)
      return false
    }
  }
  return true
}

export function allImportedEntriesMatchExactly ({
  dir,
  filenames,
  symlinks,
  keepModulesDir,
}: {
  dir: string
  filenames: Map<string, string>
  symlinks: Map<string, string> | undefined
  keepModulesDir: boolean
}): boolean {
  if (!allFilesMatch(dir, filenames) || !allSymlinksMatch(dir, symlinks)) return false

  const expectedEntries = collectExpectedEntries(filenames, symlinks, keepModulesDir)
  const actualEntries = new Set<string>()
  try {
    collectDirectoryEntries({ root: dir, directory: dir, entries: actualEntries, preserveNodeModules: keepModulesDir })
  } catch {
    return false
  }
  return actualEntries.size === expectedEntries.size && [...expectedEntries].every(entry => actualEntries.has(entry))
}

function collectExpectedEntries (
  filenames: Map<string, string>,
  symlinks: Map<string, string> | undefined,
  preserveNodeModules: boolean
): Set<string> {
  const entries = new Set<string>()
  for (const entry of [...filenames.keys(), ...(symlinks?.keys() ?? [])]) {
    if (preserveNodeModules && (entry === 'node_modules' || entry.startsWith('node_modules/'))) continue
    entries.add(entry)
    for (let parent = path.posix.dirname(entry); parent !== '.' && parent !== '/'; parent = path.posix.dirname(parent)) {
      entries.add(parent)
    }
  }
  return entries
}

function collectDirectoryEntries ({
  root,
  directory,
  entries,
  preserveNodeModules,
}: {
  root: string
  directory: string
  entries: Set<string>
  preserveNodeModules: boolean
}): void {
  for (const dirent of fs.readdirSync(directory, { withFileTypes: true })) {
    const entryPath = path.join(directory, dirent.name)
    if (dirent.isDirectory()) {
      if (preserveNodeModules && directory === root && dirent.name === 'node_modules') continue
      entries.add(path.relative(root, entryPath).split(path.sep).join('/'))
      collectDirectoryEntries({ root, directory: entryPath, entries, preserveNodeModules })
    } else {
      entries.add(path.relative(root, entryPath).split(path.sep).join('/'))
    }
  }
}

export function symlinkMatches (dest: string, target: string): boolean {
  try {
    return fs.readlinkSync(dest) === target
  } catch {
    return false
  }
}

function fileMatches (dir: string, relativePath: string, src: string): boolean {
  const reason = mismatchReason(path.join(dir, relativePath), src)
  if (reason === undefined) return true
  globalInfo(`Re-importing "${dir}" because file "${relativePath}" ${reason}`)
  return false
}

export function mismatchReason (target: string, src: string): string | undefined {
  try {
    const targetStat = fs.lstatSync(target, { bigint: true })
    if (!targetStat.isFile()) return 'is not a regular file'
    const srcStat = gfs.statSync(src, { bigint: true })
    const storeMode = storeEntryModeForSource(src)
    const expectedMode = fileModeForImport(srcStat.mode, storeMode)
    const sameIdentity = filesHaveSameIdentity(targetStat, srcStat)
    if (
      expectedMode !== undefined &&
      (targetStat.mode & 0o777n) !== BigInt(expectedMode) &&
      (!sameIdentity || storeMode === undefined || !isLinkableStoreMode(Number(targetStat.mode), expectedMode))
    ) return 'has a different mode'
    if (sameIdentity) return undefined
    if (targetStat.size !== srcStat.size) return 'has a different size'
    if (!filesHaveEqualContents(target, src)) return 'has different content'
    return undefined
  } catch {
    return 'is missing or unreadable'
  }
}
