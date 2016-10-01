import path = require('path')
import fs = require('fs')
import pnpmPkgJson from '../pnpmPkgJson'

export type StoreDependents = {
  [name: string]: string[]
}

export type StoreDependencies = {
  [name: string]: {
    [name: string]: string
  }
}

export type StoreJson = {
  pnpm: string,
  dependents: StoreDependents,
  dependencies: StoreDependencies
}

export function read (storePath: string) {
  const storeJsonPath = path.join(storePath, 'store.json')
  try {
    return JSON.parse(fs.readFileSync(storeJsonPath, 'utf8'))
  } catch (err) {
    return {
      pnpm: pnpmPkgJson.version,
      dependents: {},
      dependencies: {}
    }
  }
}

export function save (storePath: string, storeJson: StoreJson) {
  const storeJsonPath = path.join(storePath, 'store.json')
  fs.writeFileSync(storeJsonPath, JSON.stringify(storeJson, null, 2), 'utf8')
}
