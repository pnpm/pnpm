import fs from 'fs'
import util from 'util'
import { copySync } from 'fs-extra'
import path from 'path'
import { globalWarn, logger } from '@pnpm/logger'
import { sync as rimraf } from '@zkochan/rimraf'
import { sync as makeEmptyDir } from 'make-empty-dir'
import sanitizeFilename from 'sanitize-filename'
import { fastPathTemp as pathTemp } from 'path-temp'
import renameOverwrite from 'rename-overwrite'

const filenameConflictsLogger = logger('_filename-conflicts')

export type ImportFile = (src: string, dest: string) => void

export function importIndexedDir (
  importFile: ImportFile,
  newDir: string,
  filenames: Record<string, string>,
  opts: {
    keepModulesDir?: boolean
  }
): void {
  const stage = pathTemp(newDir)
  try {
    tryImportIndexedDir(importFile, stage, filenames)
    if (opts.keepModulesDir) {
      // Keeping node_modules is needed only when the hoisted node linker is used.
      moveOrMergeModulesDirs(path.join(newDir, 'node_modules'), path.join(stage, 'node_modules'))
    }
    renameOverwrite.sync(stage, newDir)
  } catch (err: unknown) {
    try {
      rimraf(stage)
    } catch {} // eslint-disable-line:no-empty
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'EEXIST') {
      const { uniqueFileMap, conflictingFileNames } = getUniqueFileMap(filenames)
      if (Object.keys(conflictingFileNames).length === 0) throw err
      filenameConflictsLogger.debug({
        conflicts: conflictingFileNames,
        writingTo: newDir,
      })
      globalWarn(
        `Not all files were linked to "${path.relative(process.cwd(), newDir)}". ` +
        'Some of the files have equal names in different case, ' +
        'which is an issue on case-insensitive filesystems. ' +
        `The conflicting file names are: ${JSON.stringify(conflictingFileNames)}`
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
}

interface SanitizeFilenamesResult {
  sanitizedFilenames: Record<string, string>
  invalidFilenames: string[]
}

function sanitizeFilenames (filenames: Record<string, string>): SanitizeFilenamesResult {
  const sanitizedFilenames: Record<string, string> = {}
  const invalidFilenames: string[] = []
  for (const [filename, src] of Object.entries(filenames)) {
    const sanitizedFilename = filename.split('/').map((f) => sanitizeFilename(f)).join('/')
    if (sanitizedFilename !== filename) {
      invalidFilenames.push(filename)
    }
    sanitizedFilenames[sanitizedFilename] = src
  }
  return { sanitizedFilenames, invalidFilenames }
}

function tryImportIndexedDir (importFile: ImportFile, newDir: string, filenames: Record<string, string>): void {
  makeEmptyDir(newDir, { recursive: true })
  const allDirs = new Set<string>()
  Object.keys(filenames)
    .forEach((f) => {
      const dir = path.dirname(f)
      if (dir === '.') return
      allDirs.add(dir)
    })
  Array.from(allDirs)
    .sort((d1, d2) => d1.length - d2.length) // from shortest to longest
    .forEach((dir) => fs.mkdirSync(path.join(newDir, dir), { recursive: true }))
  for (const [f, src] of Object.entries(filenames)) {
    const dest = path.join(newDir, f)
    importFile(src, dest)
  }
}

interface GetUniqueFileMapResult {
  conflictingFileNames: Record<string, string>
  uniqueFileMap: Record<string, string>
}

function getUniqueFileMap (fileMap: Record<string, string>): GetUniqueFileMapResult {
  const lowercaseFiles = new Map<string, string>()
  const conflictingFileNames: Record<string, string> = {}
  const uniqueFileMap: Record<string, string> = {}
  for (const filename of Object.keys(fileMap).sort()) {
    const lowercaseFilename = filename.toLowerCase()
    if (lowercaseFiles.has(lowercaseFilename)) {
      conflictingFileNames[filename] = lowercaseFiles.get(lowercaseFilename)!
      continue
    }
    lowercaseFiles.set(lowercaseFilename, filename)
    uniqueFileMap[filename] = fileMap[filename]
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
    fs.renameSync(src, dest)
  } catch (err: unknown) {
    if (!(util.types.isNativeError(err) && 'code' in err && err.code === 'EXDEV')) throw err
    copySync(src, dest)
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
