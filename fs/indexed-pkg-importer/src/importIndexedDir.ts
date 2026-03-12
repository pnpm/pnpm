import fs from 'fs'
import util from 'util'
import fsx from 'fs-extra'
import path from 'path'
import { globalWarn, logger } from '@pnpm/logger'
import { rimrafSync } from '@zkochan/rimraf'
import { makeEmptyDirSync } from 'make-empty-dir'
import sanitizeFilename from 'sanitize-filename'
import { fastPathTemp as pathTemp } from 'path-temp'
import { renameOverwriteSync } from 'rename-overwrite'
import gfs from '@pnpm/graceful-fs'

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
  if (opts.safeToSkip && process.platform === 'win32') {
    // On Windows, renameOverwriteSync is destructive — it removes the target
    // directory when rename fails, which breaks concurrent processes that are
    // reading from it. Use a simple rename instead: if it fails, the target
    // was already placed by another process.
    try {
      fs.renameSync(stage, newDir)
      return
    } catch {} // eslint-disable-line:no-empty
    try {
      rimrafSync(stage)
    } catch {} // eslint-disable-line:no-empty
    return
  }
  try {
    renameOverwriteSync(stage, newDir)
  } catch (renameErr: unknown) {
    try {
      rimrafSync(stage)
    } catch {} // eslint-disable-line:no-empty
    // When multiple processes install the same package concurrently (e.g. parallel
    // dlx calls sharing a global virtual store), the rename can race. If the target
    // already has the expected content, another process completed the import.
    const errCode = util.types.isNativeError(renameErr) && 'code' in renameErr ? renameErr.code : undefined
    if (errCode === 'ENOTEMPTY' || errCode === 'EEXIST') {
      const firstFile = filenames.keys().next().value
      if (firstFile) {
        const targetFile = path.join(newDir, firstFile)
        for (let attempt = 0; attempt < 4; attempt++) {
          if (attempt > 0) {
            Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 50)
          }
          if (fs.existsSync(targetFile)) {
            return
          }
        }
      }
    }
    throw renameErr
  }
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
