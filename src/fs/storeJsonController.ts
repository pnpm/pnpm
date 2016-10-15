import path = require('path')
import fs = require('fs')
import pnpmPkgJson from '../pnpmPkgJson'

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

export type StoreJson = {
  pnpm: string,
  packages: StorePackageMap
}

export function read (storePath: string): StoreJson {
  const storeJsonPath = path.join(storePath, 'store.json')
  try {
    return JSON.parse(fs.readFileSync(storeJsonPath, 'utf8'))
  } catch (err) {
    return {
      pnpm: pnpmPkgJson.version,
      packages: {}
    }
  }
}

export function save (storePath: string, storeJson: StoreJson) {
  const storeJsonPath = path.join(storePath, 'store.json')
  fs.writeFileSync(storeJsonPath, JSON.stringify(storeJson, null, 2), 'utf8')
}
