import path = require('path')
import exists = require('path-exists')
import { Test } from 'tape'

export default (t: Test, storePath: string, encodedRegistryName?: string) => {
  const ern = encodedRegistryName || 'localhost+4873'
  const store = {
    async getStorePath () {
      return storePath
    },
    async getEncodedRegistryName () {
      return ern
    },
    async storeHas (pkgName: string, version?: string) {
      const pathToCheck = await store.resolve(pkgName, version)
      t.ok(await exists(pathToCheck), `${pkgName}@${version} is in store (at ${pathToCheck})`)
    },
    async storeHasNot (pkgName: string, version: string) {
      const pathToCheck = await store.resolve(pkgName, version)
      t.notOk(await exists(pathToCheck), `${pkgName}@${version} is not in store (at ${pathToCheck})`)
    },
    async resolve (pkgName: string, version?: string, relativePath?: string) {
      const pkgFolder = version ? path.join(ern, pkgName, version) : pkgName
      if (relativePath) {
        return path.join(await store.getStorePath(), pkgFolder, 'package', relativePath)
      }
      return path.join(await store.getStorePath(), pkgFolder, 'package')
    },
  }
  return store
}
