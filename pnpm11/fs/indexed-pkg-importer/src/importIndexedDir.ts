import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import gfs from '@pnpm/fs.graceful-fs'
import { globalInfo, globalWarn, logger } from '@pnpm/logger'
import type { ResolvedFrom } from '@pnpm/store.controller-types'
import { rimrafSync } from '@zkochan/rimraf'
import fsx from 'fs-extra'
import { makeEmptyDirSync } from 'make-empty-dir'
import { fastPathTemp as pathTemp } from 'path-temp'
import { renameOverwriteSync } from 'rename-overwrite'
import sanitizeFilename from 'sanitize-filename'

const filenameConflictsLogger = logger('_filename-conflicts')

export type ImportFile = (src: string, dest: string) => void

export interface Importer {
  importFile: ImportFile
  // Used for writing package.json, which is the completion marker and must
  // be written atomically.  For hard links and reflinks importFile is already
  // atomic so callers pass the same function.  The copy path passes a
  // temp-file + rename wrapper instead.
  importFileAtomic: ImportFile
}

export interface ImportIndexedDirOptions {
  keepModulesDir?: boolean
  /**
   * Whether a target that already holds this package is equivalent to the
   * import, which requires that the target path pin its contents.
   */
  safeToSkip?: boolean
  resolvedFrom?: ResolvedFrom
}

export function importIndexedDir (
  importer: Importer,
  newDir: string,
  filenames: Map<string, string>,
  opts: ImportIndexedDirOptions
): void {
  // Content-addressed target (e.g. global virtual store): the path is shared
  // across projects, so concurrent importers are expected and the directory
  // must never be removed or swapped out from under them.  It is populated in
  // place, adopted when it already matches, and otherwise repaired entry by
  // entry.  Repairing in place leaves a nested node_modules/ where it is, so
  // keepModulesDir has nothing to preserve here either.
  //
  // A local directory is copied into the target at install time, so its
  // contents can change without the lockfile changing and the path pins
  // nothing.  Such a target is rebuilt below rather than adopted or repaired.
  if (opts.safeToSkip && opts.resolvedFrom !== 'local-dir') {
    importIntoSharedDir(importer, newDir, filenames, opts)
    return
  }
  // Fast path: import directly without staging.  Callers already verified
  // the target package is missing (pkgExistsAtTargetDir / pkgLinkedToStore),
  // so we can write straight into newDir and skip the temp dir + rename.
  // On failure we fall through to the staging path, which has full error
  // handling (EEXIST dedup, ENOENT sanitized-filename retry, etc.) and
  // atomically swaps in a complete directory.
  // keepModulesDir needs the staging path to preserve the existing node_modules.
  if (!opts.keepModulesDir && tryExclusiveImport(importer, newDir, filenames)) {
    return
  }
  // Staging path: create in temp dir, then atomically rename.
  // The dir rename is itself atomic, so individual file atomicity is not
  // needed here — use importFile for everything.
  const stage = pathTemp(newDir)
  try {
    makeEmptyDirSync(stage, { recursive: true })
    tryImportIndexedDir({ importFile: importer.importFile, importFileAtomic: importer.importFile }, stage, filenames)
    if (opts.keepModulesDir) {
      // Keeping node_modules is needed only when the hoisted node linker is used.
      moveOrMergeModulesDirs(path.join(newDir, 'node_modules'), path.join(stage, 'node_modules'))
    }
  } catch (err: unknown) {
    try {
      rimrafSync(stage)
    } catch {} // eslint-disable-line:no-empty
    if (retryWithFixedFileMap(err, importer, newDir, filenames, opts)) return
    throw err
  }
  try {
    renameOverwriteSync(stage, newDir)
  } catch (renameErr: unknown) {
    try {
      rimrafSync(stage)
    } catch {} // eslint-disable-line:no-empty
    throw renameErr
  }
}

// Import into a directory whose path is shared with other projects, and with
// the installs running in them. Nothing here removes a dirent: the winner of
// the exclusive mkdir writes the package, and everyone else adopts the result
// when it already matches and otherwise replaces only the entries that do not,
// so concurrent importers converge on the same tree.
//
// Replacing rather than adopting is what keeps a shared slot repairable. The
// linking tiers report EEXIST for a dirent that is already there, so an import
// that adopts what it finds keeps a file truncated by an interrupted copy, and
// then puts the completion marker on top of it — after which the directory
// looks finished to every later install and is never repaired.
function importIntoSharedDir (
  importer: Importer,
  newDir: string,
  filenames: Map<string, string>,
  opts: ImportIndexedDirOptions
): void {
  fs.mkdirSync(path.dirname(newDir), { recursive: true })
  let created = false
  try {
    fs.mkdirSync(newDir)
    created = true
  } catch (err) {
    if (!util.types.isNativeError(err) || !('code' in err) || err.code !== 'EEXIST') throw err
  }
  if (created) {
    try {
      tryImportIndexedDir(importer, newDir, filenames)
      return
    } catch (err: unknown) {
      if (retryWithFixedFileMap(err, importer, newDir, filenames, opts)) return
      // Our own write stopped partway. Another importer may already be reading
      // what did land, so finish the directory in place instead of staging a
      // replacement for it.
    }
  }
  if (allFilesMatch(newDir, filenames)) return
  try {
    repairIndexedDir(importer, newDir, filenames)
  } catch (err: unknown) {
    if (retryWithFixedFileMap(err, importer, newDir, filenames, opts)) return
    throw err
  }
}

// Bring an occupied shared directory up to date with the file map, entry by
// entry. Files the package does not declare are left alone: a build output
// belongs to whoever put it there, and a slot other installs are reading is
// not somewhere to delete from speculatively.
function repairIndexedDir (
  importer: Importer,
  newDir: string,
  filenames: Map<string, string>
): void {
  makeFileMapDirs(newDir, filenames, { clearBlockingDirents: true })
  let packageJsonSrc: string | undefined
  for (const [f, src] of filenames) {
    if (f === 'package.json') {
      packageJsonSrc = src
      continue
    }
    replaceFileIfDifferent(importer.importFile, src, path.join(newDir, f))
  }
  if (packageJsonSrc !== undefined) {
    replaceFileIfDifferent(importer.importFile, packageJsonSrc, path.join(newDir, 'package.json'))
  }
}

// Swap the file in through a temp sibling: a reader sees either the old dirent
// or the new one, and the rename replaces what the linking tiers would have
// refused to overwrite.
function replaceFileIfDifferent (importFile: ImportFile, src: string, dest: string): void {
  if (mismatchReason(dest, src) === undefined) return
  const tmp = pathTemp(dest)
  try {
    importFile(src, tmp)
  } catch (err) {
    try {
      fs.unlinkSync(tmp)
    } catch {} // eslint-disable-line:no-empty
    throw err
  }
  clearDirentBlockingFile(dest)
  renameOverwriteSync(tmp, dest)
}

// A rename cannot put a file where a directory is (EISDIR), so one standing in
// the way has to go first. Only a damaged tree has one.
function clearDirentBlockingFile (dest: string): void {
  let stats
  try {
    stats = fs.lstatSync(dest)
  } catch (err) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return
    throw err
  }
  if (stats.isDirectory()) {
    rimrafSync(dest)
  }
}

// A file where the package needs a directory turns up only in a damaged tree,
// and would fail the mkdir below. Walking top-down means a segment whose
// parent is itself a file is never reached: the parent goes first, and
// everything under a missing segment is missing too.
function clearDirentsBlockingDir (newDir: string, relativeDir: string): void {
  let dir = newDir
  for (const segment of relativeDir.split(/[\\/]/)) {
    dir = path.join(dir, segment)
    let stats
    try {
      stats = fs.lstatSync(dir)
    } catch (err) {
      if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return
      throw err
    }
    if (stats.isDirectory()) continue
    fs.unlinkSync(dir)
    return
  }
}

// The two failures an indexed file map can cause on its own: names that
// collide on a case-insensitive filesystem, and names the filesystem rejects
// outright. Both are recovered by rewriting the map and importing again.
// Returns false for anything else, which the caller must treat as a real
// failure.
function retryWithFixedFileMap (
  err: unknown,
  importer: Importer,
  newDir: string,
  filenames: Map<string, string>,
  opts: ImportIndexedDirOptions
): boolean {
  if (!util.types.isNativeError(err) || !('code' in err)) return false
  if (err.code === 'EEXIST') {
    const { uniqueFileMap, conflictingFileNames } = getUniqueFileMap(filenames)
    if (conflictingFileNames.size === 0) return false
    filenameConflictsLogger.debug({
      conflicts: Object.fromEntries(conflictingFileNames),
      writingTo: newDir,
    })
    globalWarn(
      `Not all files were linked to "${path.relative(process.cwd(), newDir)}". ` +
      'Some of the files have equal names in different case, ' +
      'which is an issue on case-insensitive filesystems. ' +
      `The conflicting file names are: ${JSON.stringify(Object.fromEntries(conflictingFileNames))}`
    )
    importIndexedDir(importer, newDir, uniqueFileMap, opts)
    return true
  }
  if (err.code === 'ENOENT') {
    return retryWithSanitizedFilenames(importer, newDir, filenames, opts)
  }
  return false
}

// Fast path for the regular virtual store: write directly into newDir, but
// only when we can create it exclusively. A successful exclusive mkdir proves
// no other process is importing the same package concurrently, so the direct
// (non-atomic) write is safe. If newDir already exists — a concurrent importer
// or a stale partial directory from an interrupted import — this returns false
// so the caller falls back to the staging path, which builds a complete
// directory in a private temp dir and atomically renames it into place.
//
// Destructively emptying a directory another process may be populating could
// otherwise leave a partial package behind: if the surviving files include the
// package.json completion marker, every later install treats the broken
// directory as complete and never repairs it.
function tryExclusiveImport (
  importer: Importer,
  newDir: string,
  filenames: Map<string, string>
): boolean {
  fs.mkdirSync(path.dirname(newDir), { recursive: true })
  try {
    fs.mkdirSync(newDir)
  } catch (err) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'EEXIST') return false
    throw err
  }
  // We exclusively created newDir, so no other process writes into it directly
  // (concurrent importers see EEXIST above and take the staging path). The
  // directory is ours: on failure we remove our partial result before falling
  // back to staging, so the next attempt — including this process's own method
  // fallbacks (clone → hardlink → copy) — can fast-path again.
  try {
    tryImportIndexedDir(importer, newDir, filenames)
    return true
  } catch {
    try {
      rimrafSync(newDir)
    } catch {} // eslint-disable-line:no-empty
    return false
  }
}

function allFilesMatch (dir: string, filenames: Map<string, string>): boolean {
  for (const [f, src] of filenames) {
    const reason = mismatchReason(path.join(dir, f), src)
    if (reason !== undefined) {
      globalInfo(`Re-importing "${dir}" because file "${f}" ${reason}`)
      return false
    }
  }
  return true
}

// Why `target` is not the store file at `src`, or undefined when it already
// is. Files imported by hardlink or reflink share the store inode, which
// settles it without a read; the copy tier compares size first, then content.
function mismatchReason (target: string, src: string): string | undefined {
  try {
    const targetStat = gfs.statSync(target)
    const srcStat = gfs.statSync(src)
    if (targetStat.ino === srcStat.ino && targetStat.dev === srcStat.dev) return undefined
    if (targetStat.size !== srcStat.size) return 'has a different size'
    if (!gfs.readFileSync(target).equals(gfs.readFileSync(src))) return 'has different content'
    return undefined
  } catch {
    return 'is missing or unreadable'
  }
}

function retryWithSanitizedFilenames (
  importer: Importer,
  newDir: string,
  filenames: Map<string, string>,
  opts: ImportIndexedDirOptions
): boolean {
  const { sanitizedFilenames, invalidFilenames } = sanitizeFilenames(filenames)
  if (invalidFilenames.length === 0) return false
  globalWarn(`\
The package linked to "${path.relative(process.cwd(), newDir)}" had \
files with invalid names: ${invalidFilenames.join(', ')}. \
They were renamed.`)
  importIndexedDir(importer, newDir, sanitizedFilenames, opts)
  return true
}

interface SanitizeFilenamesResult {
  sanitizedFilenames: Map<string, string>
  invalidFilenames: string[]
}

function sanitizeFilenames (filenames: Map<string, string>): SanitizeFilenamesResult {
  const sanitizedFilenames = new Map<string, string>()
  const invalidFilenames: string[] = []
  for (const [filename, src] of filenames) {
    const sanitizedFilename = filename.split('/').map((f) => sanitizeFilename(f)).join('/')
    if (sanitizedFilename !== filename) {
      invalidFilenames.push(filename)
    }
    sanitizedFilenames.set(sanitizedFilename, src)
  }
  return { sanitizedFilenames, invalidFilenames }
}

function tryImportIndexedDir (
  { importFile, importFileAtomic }: Importer,
  newDir: string,
  filenames: Map<string, string>
): void {
  makeFileMapDirs(newDir, filenames)
  // Write package.json last so it acts as a completion marker.
  // pkgExistsAtTargetDir() checks for package.json to decide if a package
  // is already imported — writing it last ensures a crash mid-import won't
  // leave a partially-populated directory that appears fully imported.
  let packageJsonSrc: string | undefined
  for (const [f, src] of filenames) {
    if (f === 'package.json') {
      packageJsonSrc = src
      continue
    }
    importFile(src, path.join(newDir, f))
  }
  if (packageJsonSrc !== undefined) {
    importFileAtomic(packageJsonSrc, path.join(newDir, 'package.json'))
  }
}

// Sorting shortest-first means the recursive mkdir for a deeper directory
// always finds its ancestor already on disk.
function makeFileMapDirs (
  newDir: string,
  filenames: Map<string, string>,
  opts?: { clearBlockingDirents: boolean }
): void {
  const allDirs = new Set<string>()
  for (const f of filenames.keys()) {
    const dir = path.dirname(f)
    if (dir === '.') continue
    allDirs.add(dir)
  }
  for (const dir of Array.from(allDirs).sort((d1, d2) => d1.length - d2.length)) {
    if (opts?.clearBlockingDirents) {
      clearDirentsBlockingDir(newDir, dir)
    }
    fs.mkdirSync(path.join(newDir, dir), { recursive: true })
  }
}

interface GetUniqueFileMapResult {
  conflictingFileNames: Map<string, string>
  uniqueFileMap: Map<string, string>
}

function getUniqueFileMap (fileMap: Map<string, string>): GetUniqueFileMapResult {
  const lowercaseFiles = new Map<string, string>()
  const conflictingFileNames = new Map<string, string>()
  const uniqueFileMap = new Map<string, string>()
  for (const filename of Array.from(fileMap.keys()).sort()) {
    const lowercaseFilename = filename.toLowerCase()
    if (lowercaseFiles.has(lowercaseFilename)) {
      conflictingFileNames.set(filename, lowercaseFiles.get(lowercaseFilename)!)
      continue
    }
    lowercaseFiles.set(lowercaseFilename, filename)
    uniqueFileMap.set(filename, fileMap.get(filename)!)
  }
  return {
    conflictingFileNames,
    uniqueFileMap,
  }
}

function moveOrMergeModulesDirs (src: string, dest: string): void {
  try {
    renameEvenAcrossDevices(src, dest)
  } catch (err: unknown) {
    switch (util.types.isNativeError(err) && 'code' in err && err.code) {
      case 'ENOENT':
      // If src directory doesn't exist, there is nothing to do
        return
      case 'ENOTEMPTY':
      case 'EPERM': // This error code is thrown on Windows
      // The newly added dependency might have node_modules if it has bundled dependencies.
        mergeModulesDirs(src, dest)
        return
      default:
        throw err
    }
  }
}

function renameEvenAcrossDevices (src: string, dest: string): void {
  try {
    gfs.renameSync(src, dest)
  } catch (err: unknown) {
    if (!(util.types.isNativeError(err) && 'code' in err && err.code === 'EXDEV')) throw err
    fsx.copySync(src, dest)
  }
}

function mergeModulesDirs (src: string, dest: string): void {
  const srcFiles = fs.readdirSync(src)
  const destFiles = new Set(fs.readdirSync(dest))
  const filesToMove = srcFiles.filter((file) => !destFiles.has(file))
  for (const file of filesToMove) {
    renameEvenAcrossDevices(path.join(src, file), path.join(dest, file))
  }
}
