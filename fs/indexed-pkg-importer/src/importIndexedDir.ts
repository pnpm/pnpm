import { promises as fs } from 'fs'
import path from 'path'
import { globalWarn, logger } from '@pnpm/logger'
import rimraf from '@zkochan/rimraf'
import sanitizeFilename from 'sanitize-filename'
import makeEmptyDir from 'make-empty-dir'
import pathTemp from 'path-temp'
import renameOverwrite from 'rename-overwrite'

const filenameConflictsLogger = logger('_filename-conflicts')

export type ImportFile = (src: string, dest: string) => Promise<void>

export async function importIndexedDir (
  importFile: ImportFile,
  newDir: string,
  filenames: Record<string, string>,
  opts: {
    keepModulesDir?: boolean
  }
) {
  const stage = pathTemp(path.dirname(newDir))
  try {
    await tryImportIndexedDir(importFile, stage, filenames)
    if (opts.keepModulesDir) {
      // Keeping node_modules is needed only when the hoisted node linker is used.
      await moveOrMergeModulesDirs(path.join(newDir, 'node_modules'), path.join(stage, 'node_modules'))
    }
    await renameOverwrite(stage, newDir)
  } catch (err: any) { // eslint-disable-line
    try {
      await rimraf(stage)
    } catch (err) {} // eslint-disable-line:no-empty
    if (err['code'] === 'EEXIST') {
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
      await importIndexedDir(importFile, newDir, uniqueFileMap, opts)
      return
    }
    if (err['code'] === 'ENOENT') {
      const { sanitizedFilenames, invalidFilenames } = sanitizeFilenames(filenames)
      if (invalidFilenames.length === 0) throw err
      globalWarn(`\
The package linked to "${path.relative(process.cwd(), newDir)}" had \
files with invalid names: ${invalidFilenames.join(', ')}. \
They were renamed.`)
      await importIndexedDir(importFile, newDir, sanitizedFilenames, opts)
      return
    }
    throw err
  }
}

function sanitizeFilenames (filenames: Record<string, string>) {
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

async function tryImportIndexedDir (importFile: ImportFile, newDir: string, filenames: Record<string, string>) {
  await makeEmptyDir(newDir, { recursive: true })
  const alldirs = new Set<string>()
  Object.keys(filenames)
    .forEach((f) => {
      const dir = path.dirname(f)
      if (dir === '.') return
      alldirs.add(dir)
    })
  await Promise.all(
    Array.from(alldirs)
      .sort((d1, d2) => d1.length - d2.length) // from shortest to longest
      .map(async (dir) => fs.mkdir(path.join(newDir, dir), { recursive: true }))
  )
  await Promise.all(
    Object.entries(filenames)
      .map(async ([f, src]: [string, string]) => {
        const dest = path.join(newDir, f)
        await importFile(src, dest)
      })
  )
}

function getUniqueFileMap (fileMap: Record<string, string>) {
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

async function moveOrMergeModulesDirs (src: string, dest: string) {
  try {
    await fs.rename(src, dest)
  } catch (err: any) { // eslint-disable-line
    switch (err.code) {
    case 'ENOENT':
      // If src directory doesn't exist, there is nothing to do
      return
    case 'ENOTEMPTY':
    case 'EPERM': // This error code is thrown on Windows
      // The newly added dependency might have node_modules if it has bundled dependencies.
      await mergeModulesDirs(src, dest)
      return
    default:
      throw err
    }
  }
}

async function mergeModulesDirs (src: string, dest: string) {
  const srcFiles = await fs.readdir(src)
  const destFiles = new Set(await fs.readdir(dest))
  const filesToMove = srcFiles.filter((file) => !destFiles.has(file))
  await Promise.all(filesToMove.map((file) => fs.rename(path.join(src, file), path.join(dest, file))))
}
