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
  if (opts.safeToSkip) {
    // Content-addressable target (e.g. global virtual store): the path includes
    // a content hash, so if the target already exists it was placed by another
    // process and has the correct content.
    try {
      fs.renameSync(stage, newDir)
      return
    } catch (err: unknown) {
      const errCode = util.types.isNativeError(err) && 'code' in err ? err.code : undefined
      if (process.platform === 'win32') {
        // On Windows, never fall through to renameOverwriteSync — its
        // rimrafSync(target) fails with EPERM when another process has files
        // open, breaking concurrent GVS access. Trust the hash instead.
        try {
          rimrafSync(stage)
        } catch {} // eslint-disable-line:no-empty
        return
      }
      // On POSIX, renameOverwriteSync is safe (unlink works with open handles).
      // Check content for diagnostics before falling through.
      const diag = getContentMismatchDiag(newDir, filenames)
      console.warn(`[importIndexedDir] rename to "${newDir}" failed (${errCode}). ${diag}`)
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

function getContentMismatchDiag (dir: string, filenames: Map<string, string>): string {
  const lines: string[] = []
  for (const [f, src] of filenames) {
    const target = path.join(dir, f)
    try {
      const targetStat = gfs.statSync(target)
      const srcStat = gfs.statSync(src)
      if (targetStat.ino === srcStat.ino && targetStat.dev === srcStat.dev) continue
      if (targetStat.size !== srcStat.size) {
        lines.push(`"${f}" size mismatch: target=${targetStat.size}, source=${srcStat.size}`)
        return lines.join('; ')
      }
      if (!gfs.readFileSync(target).equals(gfs.readFileSync(src))) {
        lines.push(`"${f}" content mismatch despite same size`)
        return lines.join('; ')
      }
    } catch (err: unknown) {
      const code = util.types.isNativeError(err) && 'code' in err ? err.code : undefined
      lines.push(`"${f}" unreadable (${code})`)
      return lines.join('; ')
    }
  }
  return 'all files match — target already exists'
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
