import fs from 'node:fs'
import path from 'node:path'

import { applyHunks } from './applyHunks.js'
import type { FileCreation, FileDeletion, FileModeChange, FileRename, PatchFilePart } from './parsePatch.js'

export interface ExecuteEffectsOpts {
  /** The directory the patch's paths are relative to. */
  root: string
  /** Check that every effect applies without changing any file. */
  dryRun: boolean
}

/**
 * Apply each effect of a parsed patch in order. An effect that fails
 * throws, leaving the effects before it applied.
 */
export function executeEffects (effects: PatchFilePart[], opts: ExecuteEffectsOpts): void {
  const resolve = (filePath: string): string => path.join(opts.root, filePath)
  for (const effect of effects) {
    switch (effect.type) {
      case 'file deletion':
        deleteFile(effect, resolve(effect.path), opts.dryRun)
        break
      case 'rename':
        renameFile(effect, resolve, opts.dryRun)
        break
      case 'file creation':
        createFile(effect, resolve(effect.path), opts.dryRun)
        break
      case 'patch':
        applyHunks(effect, resolve(effect.path), opts.dryRun)
        break
      case 'mode change':
        changeMode(effect, resolve(effect.path), opts.dryRun)
        break
    }
  }
}

function deleteFile (effect: FileDeletion, filePath: string, dryRun: boolean): void {
  if (!dryRun) {
    fs.unlinkSync(filePath)
  } else if (!fs.existsSync(filePath)) {
    throw new Error(`Trying to delete file that doesn't exist: ${effect.path}`)
  }
}

function renameFile (effect: FileRename, resolve: (filePath: string) => string, dryRun: boolean): void {
  const fromPath = resolve(effect.fromPath)
  const toPath = resolve(effect.toPath)
  if (!fs.existsSync(fromPath)) {
    throw new Error(`Trying to move file that doesn't exist: ${effect.fromPath}`)
  }
  if (fs.existsSync(toPath) && !isRespelling(fromPath, toPath)) {
    throw new Error(`Trying to move a file onto one that already exists: ${effect.toPath}`)
  }
  if (dryRun) return
  fs.mkdirSync(path.dirname(toPath), { recursive: true })
  fs.renameSync(fromPath, toPath)
}

/**
 * Whether `toPath` names the file at `fromPath` with only its case or Unicode
 * normalization changed. On a case-insensitive file system such a destination
 * exists before the rename, because it is the source file.
 */
function isRespelling (fromPath: string, toPath: string): boolean {
  const fromName = path.basename(fromPath)
  const toName = path.basename(toPath)
  if (fromName === toName || normalizeName(fromName) !== normalizeName(toName)) return false
  // lstat: a symlink and the file it points to are different entries. bigint:
  // NTFS file IDs do not fit a double.
  const fromStat = fs.lstatSync(fromPath, { bigint: true })
  const toStat = fs.lstatSync(toPath, { bigint: true })
  return fromStat.dev === toStat.dev && fromStat.ino === toStat.ino
}

function normalizeName (name: string): string {
  return name.toLowerCase().normalize('NFC')
}

function createFile (effect: FileCreation, filePath: string, dryRun: boolean): void {
  if (dryRun) {
    if (fs.existsSync(filePath)) {
      throw new Error(`Trying to create file that already exists: ${effect.path}`)
    }
    return
  }
  const firstPart = effect.hunk?.parts[0]
  const contents = firstPart == null
    ? ''
    : firstPart.lines.join('\n') + (firstPart.noNewlineAtEndOfFile ? '' : '\n')
  fs.mkdirSync(path.dirname(filePath), { recursive: true })
  fs.writeFileSync(filePath, contents, { mode: effect.mode })
}

function changeMode (effect: FileModeChange, filePath: string, dryRun: boolean): void {
  if (!dryRun) {
    fs.chmodSync(filePath, effect.newMode)
  } else if (!fs.existsSync(filePath)) {
    throw new Error(`Trying to change the mode of a file that doesn't exist: ${effect.path}`)
  }
}
