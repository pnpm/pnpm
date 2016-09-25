import path = require('path')
import fs = require('fs')

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

export type StoreJsonCtrl = {
  read(): StoreJson,
  save(storeJson: StoreJson): void
}

export default function storeJsonController (storePath: string): StoreJsonCtrl {
  const storeJsonPath = path.join(storePath, 'store.json')

  return {
    read () {
      try {
        return JSON.parse(fs.readFileSync(storeJsonPath, 'utf8'))
      } catch (err) {
        return null
      }
    },
    save (storeJson: StoreJson) {
      fs.writeFileSync(storeJsonPath, JSON.stringify(storeJson, null, 2), 'utf8')
    }
  }
}
