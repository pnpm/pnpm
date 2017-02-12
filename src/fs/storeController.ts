import path = require('path')
import loadYamlFile = require('load-yaml-file')
import writeYamlFile = require('write-yaml-file')

const STORE_YAML = 'store.yaml'

export type Store = {
  [name: string]: string[],
}

export async function read (storePath: string): Promise<Store | null> {
  const storeYamlPath = path.join(storePath, STORE_YAML)
  try {
    return await loadYamlFile<Store>(storeYamlPath)
  } catch (err) {
    if ((<NodeJS.ErrnoException>err).code !== 'ENOENT') {
      throw err
    }
    return null
  }
}

export function save (storePath: string, store: Store) {
  const storeYamlPath = path.join(storePath, STORE_YAML)
  return writeYamlFile(storeYamlPath, store, {sortKeys: true})
}
