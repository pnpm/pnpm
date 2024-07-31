import fs from 'fs'
import path from 'path'
import util from 'util'

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

export interface DeleteEditDirStateOptions extends ReadEditDirStateOptions {}

export function deleteEditDirState (opts: DeleteEditDirStateOptions): void {
  modifyStateFile(opts.modulesDir, state => {
    const key = createEditDirKey(opts)
    delete state[key]
  })
}

function modifyStateFile (modulesDir: string, modifyState: (state: State) => void): void {
  const filePath = getStateFilePath(modulesDir)
  let state = readStateFile(modulesDir)
  if (!state) {
    state = {}
    fs.mkdirSync(path.dirname(filePath), { recursive: true })
  }
  modifyState(state)
  fs.writeFileSync(filePath, JSON.stringify(state, undefined, 2))
}

function readStateFile (modulesDir: string): State | undefined {
  let fileContent: string
  try {
    fileContent = fs.readFileSync(getStateFilePath(modulesDir), 'utf-8')
  } catch (err) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return undefined
    }
    throw err
  }
  return JSON.parse(fileContent)
}

function getStateFilePath (modulesDir: string): string {
  return path.join(modulesDir, '.pnpm_patches', 'state.json')
}
