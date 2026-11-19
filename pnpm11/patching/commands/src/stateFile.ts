import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'

import { isError } from '@pnpm/error'

export type EditDir = string & { __brand: 'patch-edit-dir' }

export interface EditDirState {
  patchedPkg: string
  applyToAll: boolean
}

export type State = Record<EditDir, EditDirState>

export interface EditDirKeyInput {
  editDir: string
}

const createEditDirKey = (opts: EditDirKeyInput): EditDir => opts.editDir as EditDir

export interface ReadEditDirStateOptions extends EditDirKeyInput {
  modulesDir: string
}

export function readEditDirState (opts: ReadEditDirStateOptions): EditDirState | undefined {
  const state = readStateFile(opts.modulesDir)
  if (!state) return undefined
  const key = createEditDirKey(opts)
  return state[key]
}

export interface WriteEditDirStateOptions extends ReadEditDirStateOptions, EditDirState {}

export function writeEditDirState (opts: WriteEditDirStateOptions): void {
  modifyStateFile(opts.modulesDir, state => {
    const key = createEditDirKey(opts)
    state[key] = {
      patchedPkg: opts.patchedPkg,
      applyToAll: opts.applyToAll,
    }
  })
}

export interface DeleteEditDirStatesOptions {
  modulesDir: string
  patchedPkgs: string[]
  editDirs: string[]
}

/**
 * Removes the entries recorded for any of the given packages or edit
 * directories, and returns the edit directories of the removed entries.
 */
export function deleteEditDirStates (opts: DeleteEditDirStatesOptions): EditDir[] {
  const patchedPkgs = new Set(opts.patchedPkgs)
  const editDirs = new Set(opts.editDirs.map((editDir) => path.resolve(editDir)))
  const removed: EditDir[] = []
  modifyStateFile(opts.modulesDir, state => {
    for (const [key, entry] of Object.entries(state) as Array<[EditDir, EditDirState]>) {
      if (patchedPkgs.has(entry.patchedPkg) || editDirs.has(path.resolve(key))) {
        delete state[key]
        removed.push(key)
      }
    }
  })
  return removed
}

function modifyStateFile (modulesDir: string, modifyState: (state: State) => void): void {
  const filePath = getStateFilePath(modulesDir)
  const state = readStateFile(modulesDir) ?? {}
  modifyState(state)
  if (Object.keys(state).length) {
    writeStateFile(filePath, state)
  } else if (fs.existsSync(filePath)) {
    removeStateFile(filePath)
  }
}

function writeStateFile (filePath: string, state: State): void {
  fs.mkdirSync(path.dirname(filePath), { recursive: true })
  const tmpPath = `${filePath}.${crypto.randomUUID()}.tmp`
  try {
    fs.writeFileSync(tmpPath, JSON.stringify(state, undefined, 2), { flag: 'wx' })
    fs.renameSync(tmpPath, filePath)
  } catch (err) {
    fs.rmSync(tmpPath, { force: true })
    throw err
  }
}

function removeStateFile (filePath: string): void {
  fs.unlinkSync(filePath)
  try {
    fs.rmdirSync(path.dirname(filePath))
  } catch (err: unknown) {
    if (!isError(err) || !('code' in err) || (err.code !== 'ENOENT' && err.code !== 'ENOTEMPTY')) throw err
  }
}

export function readStateFile (modulesDir: string): State | undefined {
  let fileContent: string
  try {
    fileContent = fs.readFileSync(getStateFilePath(modulesDir), 'utf-8')
  } catch (err) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') {
      return undefined
    }
    throw err
  }
  return JSON.parse(fileContent)
}

function getStateFilePath (modulesDir: string): string {
  return path.join(modulesDir, '.pnpm_patches', 'state.json')
}
