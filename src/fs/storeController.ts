import path = require('path')
import pnpmPkgJson from '../pnpmPkgJson'
import {
  read as readYaml,
  write as writeYaml
} from './yamlfs'

const storeFileName = 'store.yaml'

export type StorePackageMap = {
  [name: string]: StorePackage
}

export type StorePackage = {
  dependents: string[],
  dependencies: DependenciesResolution
}

export type DependenciesResolution = {
  [name: string]: string
}

export type Store = {
  pnpm: string,
  type: TreeType,
  packages: StorePackageMap
}

export type TreeType = 'flat' | 'nested'

export function create (treeType: TreeType): Store {
  return {
    pnpm: pnpmPkgJson.version,
    type: treeType,
    packages: {}
  }
}

export async function read (storePath: string): Promise<Store | null> {
  const storeYamlPath = path.join(storePath, storeFileName)
  try {
    return await readYaml<Store>(storeYamlPath)
  } catch (err) {
    if ((<NodeJS.ErrnoException>err).code !== 'ENOENT') {
      throw err
    }
    return null
  }
}

export function save (storePath: string, store: Store) {
  const storeYamlPath = path.join(storePath, storeFileName)
  return writeYaml(storeYamlPath, store)
}
