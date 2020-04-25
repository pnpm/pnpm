import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import path = require('path')
import exists = require('path-exists')
import { Test } from 'tape'

export default (t: Test, storePath: string | Promise<string>, encodedRegistryName?: string) => {
  const ern = encodedRegistryName || `localhost+${REGISTRY_MOCK_PORT}`
  const store = {
    async storeHas (pkgName: string, version?: string): Promise<void> {
      const pathToCheck = await store.resolve(pkgName, version)
      t.ok(await exists(pathToCheck), `${pkgName}@${version} is in store (at ${pathToCheck})`)
    },
    async storeHasNot (pkgName: string, version?: string): Promise<void> {
      const pathToCheck = await store.resolve(pkgName, version)
      t.notOk(await exists(pathToCheck), `${pkgName}@${version} is not in store (at ${pathToCheck})`)
    },
    async resolve (pkgName: string, version?: string, relativePath?: string): Promise<string> {
      const pkgFolder = version ? path.join(ern, pkgName, version) : pkgName
      if (relativePath) {
        return path.join(await storePath, pkgFolder, relativePath)
      }
      return path.join(await storePath, pkgFolder)
    },
  }
  return store
}
