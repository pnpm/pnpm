import path = require('path')
import loadJsonFile = require('load-json-file')
import writeJsonFile = require('write-json-file')

const STORE_JSON = 'store.json'

export type Store = {
  [name: string]: string[],
}

export async function read (storePath: string): Promise<Store | null> {
  const storeJsonPath = path.join(storePath, STORE_JSON)
  try {
    return await loadJsonFile(storeJsonPath)
  } catch (err) {
    if ((<NodeJS.ErrnoException>err).code !== 'ENOENT') {
      throw err
    }
    return null
  }
}

export function save (storePath: string, store: Store) {
  const storeJsonPath = path.join(storePath, STORE_JSON)
  return writeJsonFile(storeJsonPath, store)
}
