import fs from 'fs'
import path from 'path'
import util from 'util'

export type EditDir = string & { __brand: 'patch-edit-dir' }

export interface EditDirState {
  selector: string
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

export async function readEditDirState (opts: ReadEditDirStateOptions): Promise<EditDirState | undefined> {
  const state = await readStateFile(opts.modulesDir)
  if (!state) return undefined
  const key = createEditDirKey(opts)
  return state[key]
}

export interface WriteEditDirStateOptions extends ReadEditDirStateOptions {
  value: EditDirState
}

export async function writeEditDirState (opts: WriteEditDirStateOptions): Promise<void> {
  await modifyStateFile(opts.modulesDir, state => {
    const key = createEditDirKey(opts)
    state[key] = opts.value
  })
}

export interface DeleteEditDirStateOptions extends ReadEditDirStateOptions {}

export async function deleteEditDirState (opts: DeleteEditDirStateOptions): Promise<void> {
  await modifyStateFile(opts.modulesDir, state => {
    const key = createEditDirKey(opts)
    delete state[key]
  })
}

export async function modifyStateFile (modulesDir: string, modifyState: (state: State) => void): Promise<void> {
  const filePath = getStateFilePath(modulesDir)
  let state = await readStateFile(modulesDir)
  if (!state) {
    state = {}
    await fs.promises.mkdir(path.dirname(filePath), { recursive: true })
  }
  modifyState(state)
  await fs.promises.writeFile(filePath, JSON.stringify(state, undefined, 2))
}

async function readStateFile (modulesDir: string): Promise<State | undefined> {
  let fileContent: string
  try {
    fileContent = await fs.promises.readFile(getStateFilePath(modulesDir), 'utf-8')
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
