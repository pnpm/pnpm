import pnpmLogger, { globalWarn } from '@pnpm/logger'
import path = require('path')
import rimraf = require('@zkochan/rimraf')
import makeEmptyDir = require('make-empty-dir')
import fs = require('mz/fs')
import pathTemp = require('path-temp')
import renameOverwrite = require('rename-overwrite')

const filenameConflictsLogger = pnpmLogger('_filename-conflicts')

export type ImportFile = (src: string, dest: string) => Promise<void>

export default async function importIndexedDir (
  importFile: ImportFile,
  newDir: string,
  filenames: Record<string, string>
) {
  const stage = pathTemp(path.dirname(newDir))
  try {
    await tryImportIndexedDir(importFile, stage, filenames)
    await renameOverwrite(stage, newDir)
  } catch (err) {
    try {
      await rimraf(stage)
    } catch (err) {} // eslint-disable-line:no-empty
    if (err['code'] !== 'EEXIST') throw err

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
    await importIndexedDir(importFile, newDir, uniqueFileMap)
  }
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
      .map((dir) => fs.mkdir(path.join(newDir, dir), { recursive: true }))
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
  const conflictingFileNames = {}
  const uniqueFileMap = {}
  for (const filename of Object.keys(fileMap).sort()) {
    const lowercaseFilename = filename.toLowerCase()
    if (lowercaseFiles.has(lowercaseFilename)) {
      conflictingFileNames[filename] = lowercaseFiles.get(lowercaseFilename)
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
