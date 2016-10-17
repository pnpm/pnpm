import path = require('path')
import fs = require('fs')
import pnpmPkgJson from '../pnpmPkgJson'
import yaml = require('js-yaml')

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
  packages: StorePackageMap
}

export function read (storePath: string): Store {
  const storeYamlPath = path.join(storePath, 'store.yaml')
  try {
    return yaml.safeLoad(fs.readFileSync(storeYamlPath, 'utf8'))
  } catch (err) {
    return {
      pnpm: pnpmPkgJson.version,
      packages: {}
    }
  }
}

export function save (storePath: string, store: Store) {
  const storeYamlPath = path.join(storePath, 'store.yaml')
  fs.writeFileSync(storeYamlPath, yaml.safeDump(store), 'utf8')
}
