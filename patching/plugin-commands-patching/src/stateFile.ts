import fs from 'fs'
import path from 'path'
import util from 'util'

export type StateKey = string

export interface StateValue {
  selector: string
  applyToAll: boolean
}

export type State = Record<StateKey, StateValue>

export interface StateKeyInput {
  editDir: string
}

const createStateKey = (opts: StateKeyInput): StateKey => opts.editDir

export interface ReadStateValueOptions extends StateKeyInput {
  modulesDir: string
}

export async function readStateValue (opts: ReadStateValueOptions): Promise<StateValue | undefined> {
  const state = await readStateFile(opts.modulesDir)
  if (!state) return undefined
  const key = createStateKey(opts)
  return state[key]
}

export interface WriteStateValueOptions extends ReadStateValueOptions {
  value: StateValue
}

export async function writeStateValue (opts: WriteStateValueOptions): Promise<void> {
  await modifyStateFile(opts.modulesDir, state => {
    const key = createStateKey(opts)
    state[key] = opts.value
  })
}

export interface DeleteStateKeyOptions extends ReadStateValueOptions {}

export async function deleteStateKey (opts: DeleteStateKeyOptions): Promise<void> {
  await modifyStateFile(opts.modulesDir, state => {
    const key = createStateKey(opts)
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
