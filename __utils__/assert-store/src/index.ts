import '@total-typescript/ts-reset'
import path from 'node:path'

import exists from 'path-exists'

import { getFilePathInCafs } from '@pnpm/store.cafs'
import { getIntegrity, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'

export function assertStore(
  storePath: string | Promise<string>,
  encodedRegistryName?: string | undefined
) {
  const ok = (value: unknown) => expect(value).toBeTruthy()

  const notOk = (value: unknown) => expect(value).toBeFalsy()

  const ern = encodedRegistryName ?? `localhost+${REGISTRY_MOCK_PORT}`

  const store = {
    async getPkgIndexFilePath(
      pkgName: string,
      version?: string | undefined
    ): Promise<string> {
      const cafsDir = path.join(await storePath, 'files')

      const integrity = version ? getIntegrity(pkgName, version) : pkgName

      return getFilePathInCafs(cafsDir, integrity, 'index')
    },
    async cafsHas(pkgName: string, version?: string | undefined): Promise<void> {
      const pathToCheck = await store.getPkgIndexFilePath(pkgName, version)

      ok(await exists(pathToCheck))
    },
    async cafsHasNot(pkgName: string, version?: string | undefined): Promise<void> {
      const pathToCheck = await store.getPkgIndexFilePath(pkgName, version)

      notOk(await exists(pathToCheck))
    },
    async storeHas(pkgName: string, version?: string | undefined): Promise<void> {
      const pathToCheck = await store.resolve(pkgName, version)

      ok(await exists(pathToCheck))
    },
    async storeHasNot(pkgName: string, version?: string | undefined): Promise<void> {
      const pathToCheck = await store.resolve(pkgName, version)

      notOk(await exists(pathToCheck))
    },
    async resolve(
      pkgName: string,
      version?: string | undefined,
      relativePath?: string | undefined
    ): Promise<string> {
      const pkgFolder = version ? path.join(ern, pkgName, version) : pkgName

      if (relativePath) {
        return path.join(await storePath, pkgFolder, relativePath)
      }

      return path.join(await storePath, pkgFolder)
    },
  }

  return store
}
