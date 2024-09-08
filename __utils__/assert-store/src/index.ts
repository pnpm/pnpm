import fs from 'fs'
import path from 'path'
import { getIndexFilePathInCafs } from '@pnpm/store.cafs'
import { getIntegrity, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'

export interface StoreAssertions {
  getPkgIndexFilePath: (pkgName: string, version?: string) => string
  cafsHas: (pkgName: string, version?: string) => void
  cafsHasNot: (pkgName: string, version?: string) => void
  storeHas: (pkgName: string, version?: string) => void
  storeHasNot: (pkgName: string, version?: string) => void
  resolve: (pkgName: string, version?: string, relativePath?: string) => string
}

export function assertStore (
  storePath: string,
  encodedRegistryName?: string
): StoreAssertions {
  // eslint-disable-next-line
  const ok = (value: any) => expect(value).toBeTruthy()
  // eslint-disable-next-line
  const notOk = (value: any) => expect(value).toBeFalsy()
  const ern = encodedRegistryName ?? `localhost+${REGISTRY_MOCK_PORT}`
  const store = {
    getPkgIndexFilePath (pkgName: string, version?: string): string {
      const cafsDir = path.join(storePath, 'files')
      const integrity = version ? getIntegrity(pkgName, version) : pkgName
      return getIndexFilePathInCafs(cafsDir, integrity)
    },
    cafsHas (pkgName: string, version?: string): void {
      const pathToCheck = store.getPkgIndexFilePath(pkgName, version)
      ok(fs.existsSync(pathToCheck))
    },
    cafsHasNot (pkgName: string, version?: string): void {
      const pathToCheck = store.getPkgIndexFilePath(pkgName, version)
      notOk(fs.existsSync(pathToCheck))
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
