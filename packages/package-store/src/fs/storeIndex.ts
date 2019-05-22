import { StoreIndex } from '@pnpm/types'
import loadJsonFile = require('load-json-file')
import path = require('path')
import writeJsonFile = require('write-json-file')

const STORE_JSON = 'store.json'

export async function read (storePath: string): Promise<StoreIndex | null> {
  const storeJsonPath = path.join(storePath, STORE_JSON)
  try {
    return await loadJsonFile<StoreIndex>(storeJsonPath)
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code !== 'ENOENT') {
      throw err
    }
    return null
  }
}

export function save (storePath: string, store: StoreIndex) {
  const storeJsonPath = path.join(storePath, STORE_JSON)
  return writeJsonFile(storeJsonPath, store)
}

export function saveSync (storePath: string, store: StoreIndex) {
  const storeJsonPath = path.join(storePath, STORE_JSON)
  return writeJsonFile.sync(storeJsonPath, store)
}
