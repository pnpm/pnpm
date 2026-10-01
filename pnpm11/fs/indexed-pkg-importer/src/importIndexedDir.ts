import fs from 'node:fs'
import path from 'node:path'

import { isError } from '@pnpm/error'
import { withFileLockRetry } from '@pnpm/fs.graceful-fs'
import { globalWarn, logger } from '@pnpm/logger'
import type { ResolvedFrom } from '@pnpm/store.controller-types'
import { rimrafSync } from '@zkochan/rimraf'
import { makeEmptyDirSync } from 'make-empty-dir'
import { fastPathTemp as pathTemp } from 'path-temp'
import { renameOverwriteSync } from 'rename-overwrite'

import { copyInternalSymlink, type SymlinkDirs } from './copyInternalSymlink.js'
import { makeFileMapDirs } from './dirUtils.js'
import {
  getUniqueFileMap,
  moveOrMergeModulesDirs,
  sanitizeFilenames,
} from './fileMapUtils.js'
import {
  allFilesMatch,
  allSymlinksMatch,
  repairIndexedDir,
} from './repairIndexedDir.js'

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
  /** Symlinks to create in the package, keyed by their path relative to it, with their targets. */
  symlinks?: Map<string, string>
}

// What one call to importIndexedDir is importing, threaded to the helpers that
// need all of it — including the retries, which re-enter with a rewritten map.
export interface IndexedDirImport {
  importer: Importer
  newDir: string
  filenames: Map<string, string>
  opts: ImportIndexedDirOptions
}

export function importIndexedDir (
  importer: Importer,
  newDir: string,
  filenames: Map<string, string>,
  opts: ImportIndexedDirOptions
): void {
  const dirImport: IndexedDirImport = { importer, newDir, filenames, opts }
  if (opts.safeToSkip && opts.resolvedFrom !== 'local-dir') {
    importIntoSharedDir(dirImport)
    return
  }
  if (!opts.keepModulesDir && tryExclusiveImport(importer, newDir, filenames, {
    links: symlinkDirs(opts, filenames, { writtenDir: newDir, finalDir: newDir }),
    symlinks: opts.symlinks,
  })) {
    return
  }
  importWithStaging(dirImport)
}

function importWithStaging (dirImport: IndexedDirImport): void {
  const { importer, newDir, filenames, opts } = dirImport
  const stage = pathTemp(newDir)
  try {
    makeEmptyDirSync(stage, { recursive: true })
    tryImportIndexedDir(
      { importFile: importer.importFile, importFileAtomic: importer.importFile },
      stage,
      filenames,
      {
        links: symlinkDirs(opts, filenames, { writtenDir: stage, finalDir: newDir }),
        symlinks: opts.symlinks,
      }
    )
    if (opts.keepModulesDir) {
      moveOrMergeModulesDirs(path.join(newDir, 'node_modules'), path.join(stage, 'node_modules'))
    }
  } catch (err: unknown) {
    cleanStageDir(stage)
    if (retryWithFixedFileMap(err, dirImport)) return
    throw err
  }
  commitStagedDir(stage, newDir)
}

function commitStagedDir (stage: string, newDir: string): void {
  try {
    withFileLockRetry(() => {
      renameOverwriteSync(stage, newDir)
    })
  } catch (renameErr: unknown) {
    cleanStageDir(stage)
    throw renameErr
  }
}

function cleanStageDir (stage: string): void {
  try {
    rimrafSync(stage)
  } catch {}
}

function tryExclusiveMkdir (newDir: string): boolean {
  fs.mkdirSync(path.dirname(newDir), { recursive: true })
  try {
    fs.mkdirSync(newDir)
    return true
  } catch (err) {
    if (isError(err) && 'code' in err && err.code === 'EEXIST') return false
    throw err
  }
}

function tryDirectSharedImport (dirImport: IndexedDirImport): boolean {
  const { importer, newDir, filenames, opts } = dirImport
  if (!tryExclusiveMkdir(newDir)) return false
  try {
    tryImportIndexedDir(importer, newDir, filenames, { symlinks: opts.symlinks })
    return true
  } catch (err: unknown) {
    return retryWithFixedFileMap(err, dirImport)
  }
}

function importIntoSharedDir (dirImport: IndexedDirImport): void {
  if (tryDirectSharedImport(dirImport)) return
  const { newDir, filenames, opts } = dirImport
  if (allFilesMatch(newDir, filenames) && allSymlinksMatch(newDir, opts.symlinks)) return
  try {
    repairIndexedDir(dirImport)
  } catch (err: unknown) {
    if (retryWithFixedFileMap(err, dirImport)) return
    throw err
  }
}

function retryWithFixedFileMap (err: unknown, dirImport: IndexedDirImport): boolean {
  const { importer, newDir, filenames, opts } = dirImport
  if (!isError(err) || !('code' in err)) return false
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
    return retryWithSanitizedFilenames(dirImport)
  }
  return false
}

function tryExclusiveImport (
  importer: Importer,
  newDir: string,
  filenames: Map<string, string>,
  entries: ImportedEntries
): boolean {
  if (!tryExclusiveMkdir(newDir)) return false
  try {
    tryImportIndexedDir(importer, newDir, filenames, entries)
    return true
  } catch {
    try {
      rimrafSync(newDir)
    } catch {}
    return false
  }
}

function retryWithSanitizedFilenames ({ importer, newDir, filenames, opts }: IndexedDirImport): boolean {
  const { sanitizedFilenames, invalidFilenames } = sanitizeFilenames(filenames)
  if (invalidFilenames.length === 0) return false
  globalWarn(`\
The package linked to "${path.relative(process.cwd(), newDir)}" had \
files with invalid names: ${invalidFilenames.join(', ')}. \
They were renamed.`)
  importIndexedDir(importer, newDir, sanitizedFilenames, opts)
  return true
}

interface ImportedEntries {
  links?: SymlinkDirs
  symlinks?: Map<string, string>
}

function tryImportIndexedDir (
  { importFile, importFileAtomic }: Importer,
  newDir: string,
  filenames: Map<string, string>,
  { links, symlinks }: ImportedEntries = {}
): void {
  makeFileMapDirs(newDir, filenames)
  let packageJsonSrc: string | undefined
  for (const [relativePath, src] of filenames) {
    if (relativePath === 'package.json') {
      packageJsonSrc = src
      continue
    }
    importEntry(importFile, src, path.join(newDir, relativePath), links)
  }
  for (const [relativePath, target] of symlinks ?? []) {
    const dest = path.join(newDir, relativePath)
    fs.mkdirSync(path.dirname(dest), { recursive: true })
    fs.symlinkSync(target, dest)
  }
  if (packageJsonSrc !== undefined) {
    importEntry(importFileAtomic, packageJsonSrc, path.join(newDir, 'package.json'), links)
  }
}

function symlinkDirs (
  opts: ImportIndexedDirOptions,
  filenames: Map<string, string>,
  dirs: Pick<SymlinkDirs, 'writtenDir' | 'finalDir'>
): SymlinkDirs | undefined {
  if (opts.resolvedFrom !== 'local-dir') return undefined
  const imported = new Set<string>([''])
  for (const relativePath of filenames.keys()) {
    for (let entry = relativePath; entry !== '' && !imported.has(entry); entry = path.posix.dirname(entry).replace(/^\.$/, '')) {
      imported.add(entry)
    }
  }
  return { ...dirs, imported }
}

function importEntry (importFile: ImportFile, src: string, dest: string, links?: SymlinkDirs): void {
  if (links != null && fs.lstatSync(src).isSymbolicLink() && copyInternalSymlink(src, dest, links)) return
  importFile(src, dest)
}
