import fs from 'node:fs'
import path from 'node:path'

import { expect } from '@jest/globals'
import { StoreIndex, storeIndexKey } from '@pnpm/store.index'
import { getIntegrity, REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'

export interface StoreAssertions {
  getPkgIndexFilePath: (pkgName: string, version: string) => string
  cafsHas: (pkgName: string, version: string) => void
  cafsHasNot: (pkgName: string, version: string) => void
  storeHas: (pkgName: string, version?: string) => void
  storeHasNot: (pkgName: string, version?: string) => void
  resolve: (pkgName: string, version?: string, relativePath?: string) => string
}

export function assertStore (
  storePath: string,
  encodedRegistryName?: string
): StoreAssertions {
  const ok = (value: unknown): void => expect(value).toBeTruthy()
  const notOk = (value: unknown): void => expect(value).toBeFalsy()
  const ern = encodedRegistryName ?? `localhost+${REGISTRY_MOCK_PORT}`
  const store = {
    getPkgIndexFilePath,
    cafsHas (pkgName: string, version: string): void {
      ok(storeIndexHas(storePath, getPkgIndexFilePath(pkgName, version)))
    },
    cafsHasNot (pkgName: string, version: string): void {
      notOk(storeIndexHas(storePath, getPkgIndexFilePath(pkgName, version)))
    },
    storeHas (pkgName: string, version?: string): void {
      const pathToCheck = store.resolve(pkgName, version)
      ok(fs.existsSync(pathToCheck))
    },
    storeHasNot (pkgName: string, version?: string): void {
      const pathToCheck = store.resolve(pkgName, version)
      notOk(fs.existsSync(pathToCheck))
    },
    resolve (pkgName: string, version?: string, relativePath?: string): string {
      const pkgFolder = version ? path.join(ern, pkgName, version) : pkgName
      if (relativePath) {
        return path.join(storePath, pkgFolder, relativePath)
      }
      return path.join(storePath, pkgFolder)
    },
  }
  return store
}

function getPkgIndexFilePath (pkgName: string, version: string): string {
  const integrity = getIntegrity(pkgName, version)
  return storeIndexKey(integrity, `${pkgName}@${version}`)
}

function storeIndexHas (storePath: string, indexKey: string): boolean {
  const storeIndex = new StoreIndex(storePath)
  try {
    return storeIndex.get(indexKey) != null
  } finally {
    storeIndex.close()
  }
}
