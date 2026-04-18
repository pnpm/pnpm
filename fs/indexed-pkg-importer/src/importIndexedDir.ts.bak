import fs from 'node:fs'
import { promises as fsPromises } from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import gfs from '@pnpm/fs.graceful-fs'
import { globalInfo, globalWarn, logger } from '@pnpm/logger'
import { rimraf, rimrafSync } from '@zkochan/rimraf'
import fsx from 'fs-extra'
import { makeEmptyDir, makeEmptyDirSync } from 'make-empty-dir'
import pLimit from 'p-limit'
import { fastPathTemp as pathTemp } from 'path-temp'
import { renameOverwrite, renameOverwriteSync } from 'rename-overwrite'
import sanitizeFilename from 'sanitize-filename'

const filenameConflictsLogger = logger('_filename-conflicts')

export type ImportFile = (src: string, dest: string) => Promise<void> | void

export interface Importer {
  importFile: ImportFile
  // Used for writing package.json, which is the completion marker and must
  // be written atomically.  For hard links and reflinks importFile is already
  // atomic so callers pass the same function.  The copy path passes a
  // temp-file + rename wrapper instead.
  importFileAtomic: ImportFile
}

export async function importIndexedDir (
  importer: Importer,
  newDir: string,
  filenames: Map<string, string>,
  opts: {
    keepModulesDir?: boolean
    safeToSkip?: boolean
  }
): Promise<void> {
  // Fast path: import directly without staging.  Callers already verified
  // the target package is missing (pkgExistsAtTargetDir / pkgLinkedToStore),
  // so we can write straight into newDir and skip the temp dir + rename.
  // On any error, clean up and fall through to the staging path which has
  // full error handling (EEXIST dedup, ENOENT sanitized-filename retry, etc.).
  // keepModulesDir needs the staging path to preserve the existing node_modules.
  if (!opts.keepModulesDir) try {
    // For safeToSkip (content-addressed GVS), use non-destructive mkdirSync
    // so concurrent importers don't wipe each other's files.
    if (opts.safeToSkip) {
      await fsPromises.mkdir(newDir, { recursive: true })
    } else {
      await makeEmptyDir(newDir, { recursive: true })
    }
    await tryImportIndexedDir(importer, newDir, filenames)
    return
  } catch (err) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'EEXIST') {
      // A concurrent importer may have completed the directory.
      // If all files match, there's nothing left to do.
      if (await allFilesMatch(newDir, filenames)) return
    }
  }
  // Staging path: create in temp dir, then atomically rename.
  // The dir rename is itself atomic, so individual file atomicity is not
  // needed here — use importFile for everything.
  const stage = pathTemp(newDir)
  try {
    await makeEmptyDir(stage, { recursive: true })
    await tryImportIndexedDir({ importFile: importer.importFile, importFileAtomic: importer.importFile }, stage, filenames)
    if (opts.keepModulesDir) {
      // Keeping node_modules is needed only when the hoisted node linker is used.
      moveOrMergeModulesDirs(path.join(newDir, 'node_modules'), path.join(stage, 'node_modules'))
    }
  } catch (err: unknown) {
    try {
      await rimraf(stage)
    } catch {} // eslint-disable-line:no-empty
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'EEXIST') {
      const { uniqueFileMap, conflictingFileNames } = getUniqueFileMap(filenames)
      if (conflictingFileNames.size === 0) throw err
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
      await importIndexedDir(importer, newDir, uniqueFileMap, opts)
      return
    }
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      if (await retryWithSanitizedFilenames(importer, newDir, filenames, opts)) return
      throw err
    }
    throw err
  }
  if (opts.safeToSkip) {
    // Content-addressable target (e.g. global virtual store): if the target
    // already exists and has all expected files, it has the correct content.
    // Skip instead of doing a swap-rename that temporarily removes the target
    // directory — which breaks junctions read by other processes.
    try {
      await fsPromises.rename(stage, newDir)
      return
    } catch (err: unknown) {
      if (util.types.isNativeError(err) && 'code' in err && (err.code === 'ENOTEMPTY' || err.code === 'EEXIST' || err.code === 'EPERM')) {
        if (await allFilesMatch(newDir, filenames)) {
          try {
            await rimraf(stage)
          } catch {} // eslint-disable-line:no-empty
          return
        }
      }
      // Files missing or other error — fall through to renameOverwrite
    }
  }
  try {
    await renameOverwrite(stage, newDir)
  } catch (renameErr: unknown) {
    try {
      await rimraf(stage)
    } catch {} // eslint-disable-line:no-empty
    throw renameErr
  }
}

async function allFilesMatch (dir: string, filenames: Map<string, string>): Promise<boolean> {
  const limit = pLimit(100)
  const results = await Promise.all(
    Array.from(filenames.entries()).map(([f, src]) => limit(async () => {
      const target = path.join(dir, f)
      try {
        const [targetStat, srcStat] = await Promise.all([
          fsPromises.stat(target),
          fsPromises.stat(src),
        ])
        // Fast path: hardlinks share the same inode
        if (targetStat.ino === srcStat.ino && targetStat.dev === srcStat.dev) return true
        // Copy path: compare size first, then content
        if (targetStat.size !== srcStat.size) {
          globalInfo(`Re-importing "${dir}" because file "${f}" has a different size`)
          return false
        }
        const [targetContent, srcContent] = await Promise.all([
          fsPromises.readFile(target),
          fsPromises.readFile(src),
        ])
        if (!targetContent.equals(srcContent)) {
          globalInfo(`Re-importing "${dir}" because file "${f}" has different content`)
          return false
        }
        return true
      } catch {
        globalInfo(`Re-importing "${dir}" because file "${f}" is missing or unreadable`)
        return false
      }
    }))
  )
  return results.every(Boolean)
}

async function retryWithSanitizedFilenames (
  importer: Importer,
  newDir: string,
  filenames: Map<string, string>,
  opts: { keepModulesDir?: boolean, safeToSkip?: boolean }
): Promise<boolean> {
  const { sanitizedFilenames, invalidFilenames } = sanitizeFilenames(filenames)
  if (invalidFilenames.length === 0) return false
  globalWarn(`\
The package linked to "${path.relative(process.cwd(), newDir)}" had \
files with invalid names: ${invalidFilenames.join(', ')}. \
They were renamed.`)
  await importIndexedDir(importer, newDir, sanitizedFilenames, opts)
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

async function tryImportIndexedDir (
  { importFile, importFileAtomic }: Importer,
  newDir: string,
  filenames: Map<string, string>
): Promise<void> {
  const allDirs = new Set<string>()
  for (const f of filenames.keys()) {
    const dir = path.dirname(f)
    if (dir === '.') continue
    allDirs.add(dir)
  }
  await Promise.all(
    Array.from(allDirs)
      .sort((d1, d2) => d1.length - d2.length) // from shortest to longest
      .map(async (dir) => fsPromises.mkdir(path.join(newDir, dir), { recursive: true }))
  )
  // Write package.json last so it acts as a completion marker.
  // pkgExistsAtTargetDir() checks for package.json to decide if a package
  // is already imported — writing it last ensures a crash mid-import won't
  // leave a partially-populated directory that appears fully imported.
  let packageJsonSrc: string | undefined
  const limit = pLimit(100)
  await Promise.all(
    Array.from(filenames.entries()).map(([f, src]) => limit(async () => {
      if (f === 'package.json') {
        packageJsonSrc = src
        return
      }
      await importFile(src, path.join(newDir, f))
    }))
  )
  if (packageJsonSrc !== undefined) {
    await importFileAtomic(packageJsonSrc, path.join(newDir, 'package.json'))
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
