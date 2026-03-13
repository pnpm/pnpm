import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import gfs from '@pnpm/graceful-fs'
import { globalInfo, globalWarn, logger } from '@pnpm/logger'
import { rimrafSync } from '@zkochan/rimraf'
import fsx from 'fs-extra'
import { makeEmptyDirSync } from 'make-empty-dir'
import { fastPathTemp as pathTemp } from 'path-temp'
import { renameOverwriteSync } from 'rename-overwrite'
import sanitizeFilename from 'sanitize-filename'

const filenameConflictsLogger = logger('_filename-conflicts')

export type ImportFile = (src: string, dest: string) => void

export function importIndexedDir (
  importFile: ImportFile,
  newDir: string,
  filenames: Map<string, string>,
  opts: {
    keepModulesDir?: boolean
    safeToSkip?: boolean
  }
): void {
  const stage = pathTemp(newDir)
  try {
    tryImportIndexedDir(importFile, stage, filenames)
    if (opts.keepModulesDir) {
      // Keeping node_modules is needed only when the hoisted node linker is used.
      moveOrMergeModulesDirs(path.join(newDir, 'node_modules'), path.join(stage, 'node_modules'))
    }
  } catch (err: unknown) {
    try {
      rimrafSync(stage)
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
      importIndexedDir(importFile, newDir, uniqueFileMap, opts)
      return
    }
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      const { sanitizedFilenames, invalidFilenames } = sanitizeFilenames(filenames)
      if (invalidFilenames.length === 0) throw err
      globalWarn(`\
The package linked to "${path.relative(process.cwd(), newDir)}" had \
files with invalid names: ${invalidFilenames.join(', ')}. \
They were renamed.`)
      importIndexedDir(importFile, newDir, sanitizedFilenames, opts)
      return
    }
    throw err
  }
  if (opts.safeToSkip) {
    // Content-addressable target (e.g. global virtual store): if the target
    // already exists and has all expected files, it has the correct content.
    // Skip instead of doing a swap-rename that temporarily removes the target
    // directory — which breaks junctions read by other processes.
    try {
      fs.renameSync(stage, newDir)
      return
    } catch (err: unknown) {
      if (util.types.isNativeError(err) && 'code' in err && (err.code === 'ENOTEMPTY' || err.code === 'EEXIST' || err.code === 'EPERM')) {
        if (allFilesMatch(newDir, filenames)) {
          try {
            rimrafSync(stage)
          } catch {} // eslint-disable-line:no-empty
          return
        }
      }
      // Files missing or other error — fall through to renameOverwriteSync
    }
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

function allFilesMatch (dir: string, filenames: Map<string, string>): boolean {
  for (const [f, src] of filenames) {
    const target = path.join(dir, f)
    try {
      const targetStat = gfs.statSync(target)
      const srcStat = gfs.statSync(src)
      // Fast path: hardlinks share the same inode
      if (targetStat.ino === srcStat.ino && targetStat.dev === srcStat.dev) continue
      // Copy path: compare size first, then content
      if (targetStat.size !== srcStat.size) {
        globalInfo(`Re-importing "${dir}" because file "${f}" has a different size`)
        return false
      }
      if (!gfs.readFileSync(target).equals(gfs.readFileSync(src))) {
        globalInfo(`Re-importing "${dir}" because file "${f}" has different content`)
        return false
      }
    } catch {
      globalInfo(`Re-importing "${dir}" because file "${f}" is missing or unreadable`)
      return false
    }
  }
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

function tryImportIndexedDir (importFile: ImportFile, newDir: string, filenames: Map<string, string>): void {
  makeEmptyDirSync(newDir, { recursive: true })
  const allDirs = new Set<string>()
  for (const f of filenames.keys()) {
    const dir = path.dirname(f)
    if (dir === '.') continue
    allDirs.add(dir)
  }
  Array.from(allDirs)
    .sort((d1, d2) => d1.length - d2.length) // from shortest to longest
    .forEach((dir) => fs.mkdirSync(path.join(newDir, dir), { recursive: true }))
  for (const [f, src] of filenames) {
    const dest = path.join(newDir, f)
    importFile(src, dest)
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
