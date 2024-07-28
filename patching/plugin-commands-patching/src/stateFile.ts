import fs from 'fs'
import path from 'path'
import util from 'util'

export type StateKey = `${string}:${string}`

export interface StateValue {
  selector: string
  applyToAll: boolean
}

export type State = Record<StateKey, StateValue>

export interface StateKeyInput {
  editDir: string
  lockfileDir: string
}

const createStateKey = (opts: StateKeyInput): StateKey => `${opts.editDir}:${opts.lockfileDir}`

export interface ReadStateValueOptions extends StateKeyInput {
  cacheDir: string
}

export async function readStateValue (opts: ReadStateValueOptions): Promise<StateValue | undefined> {
  const state = await readStateFile(opts.cacheDir)
  if (!state) return undefined
  const key = createStateKey(opts)
  return state[key]
}

export interface WriteStateValueOptions extends ReadStateValueOptions {
  value: StateValue
}

export async function writeStateValue (opts: WriteStateValueOptions): Promise<void> {
  await modifyStateFile(opts.cacheDir, state => {
    const key = createStateKey(opts)
    state[key] = opts.value
  })
}

export interface DeleteStateKeyOptions extends ReadStateValueOptions {}

export async function deleteStateKey (opts: DeleteStateKeyOptions): Promise<void> {
  await modifyStateFile(opts.cacheDir, state => {
    const key = createStateKey(opts)
    delete state[key]
  })
}

export async function modifyStateFile (cacheDir: string, modifyState: (state: State) => void): Promise<void> {
  let state = await readStateFile(cacheDir)
  if (!state) {
    state = {}
    await fs.promises.mkdir(cacheDir, { recursive: true })
  }
  modifyState(state)
  const filePath = getStateFilePath(cacheDir)
  await fs.promises.writeFile(filePath, JSON.stringify(state, undefined, 2))
}

async function readStateFile (cacheDir: string): Promise<State | undefined> {
  let fileContent: string
  try {
    fileContent = await fs.promises.readFile(getStateFilePath(cacheDir), 'utf-8')
  } catch (err) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return undefined
    }
    throw err
  }
  return JSON.parse(fileContent)
}

function getStateFilePath (cacheDir: string): string {
  return path.join(cacheDir, 'patch-state.json')
}
