import assertStore from '@pnpm/assert-store'
import {
  CURRENT_LOCKFILE,
  WANTED_LOCKFILE,
} from '@pnpm/constants'
import { Lockfile, LockfileImporter } from '@pnpm/lockfile-types'
import { Modules, read as readModules } from '@pnpm/modules-yaml'
import path = require('path')
import exists = require('path-exists')
import readYamlFile from 'read-yaml-file'
import { Test } from 'tape'
import writePkg = require('write-pkg')
import isExecutable from './isExecutable'

export { isExecutable, Modules }

export type RawLockfile = Lockfile & Partial<LockfileImporter>

export interface Project {
  requireModule: NodeRequireFunction
  has (pkgName: string): Promise<void>
  hasNot (pkgName: string): Promise<void>
  getStorePath (): Promise<string>
  resolve (pkgName: string, version?: string, relativePath?: string): Promise<string>
  storeHas (pkgName: string, version?: string): Promise<string>
  storeHasNot (pkgName: string, version?: string): Promise<void>
  isExecutable (pathToExe: string): Promise<void>
  readCurrentLockfile (): Promise<RawLockfile | null>
  readModulesManifest (): Promise<Modules | null>
  readLockfile (): Promise<RawLockfile | null>
  writePackageJson (pkgJson: object): Promise<void>
}

export default (t: Test, projectPath: string, encodedRegistryName?: string): Project => {
  const ern = encodedRegistryName || 'localhost+4873'
  const modules = path.join(projectPath, 'node_modules')

  let cachedStore: {
    storePath: string;
    storeHas (pkgName: string, version?: string | undefined): Promise<void>;
    storeHasNot (pkgName: string, version?: string | undefined): Promise<void>;
    resolve (pkgName: string, version?: string | undefined, relativePath?: string | undefined): Promise<string>
  }
  async function getStoreInstance () {
    if (!cachedStore) {
      const modulesYaml = await readModules(modules)
      if (!modulesYaml) {
        throw new Error(`Cannot find module store. No .modules.yaml found at "${modules}"`)
      }
      const storePath = modulesYaml.store
      cachedStore = {
        storePath,
        ...assertStore(t, storePath, ern),
      }
    }
    return cachedStore
  }

  return {
    requireModule (pkgName: string) {
      return require(path.join(modules, pkgName))
    },
    async has (pkgName: string) {
      t.ok(await exists(path.join(modules, pkgName)), `${pkgName} is in node_modules`)
    },
    async hasNot (pkgName: string) {
      t.notOk(await exists(path.join(modules, pkgName)), `${pkgName} is not in node_modules`)
    },
    async getStorePath () {
      const store = await getStoreInstance()
      return store.storePath
    },
    async resolve (pkgName: string, version?: string, relativePath?: string) {
      const store = await getStoreInstance()
      return store.resolve(pkgName, version, relativePath)
    },
    async storeHas (pkgName: string, version?: string) {
      const store = await getStoreInstance()
      return store.resolve(pkgName, version)
    },
    async storeHasNot (pkgName: string, version?: string) {
      try {
        const store = await getStoreInstance()
        return store.storeHasNot(pkgName, version)
      } catch (err) {
        if (err.message.startsWith('Cannot find module store')) {
          t.pass(`${pkgName}@${version} is not in store (store does not even exist)`)
          return
        }
        throw err
      }
    },
    isExecutable (pathToExe: string) {
      return isExecutable(t, path.join(modules, pathToExe))
    },
    async readCurrentLockfile () {
      try {
        return await readYamlFile(path.join(modules, '..', CURRENT_LOCKFILE)) // tslint:disable-line
      } catch (err) {
        if (err.code === 'ENOENT') return null
        throw err
      }
    },
    readModulesManifest: () => readModules(modules),
    async readLockfile () {
      try {
        return await readYamlFile(path.join(projectPath, WANTED_LOCKFILE)) // tslint:disable-line
      } catch (err) {
        if (err.code === 'ENOENT') return null
        throw err
      }
    },
    async writePackageJson (pkgJson: object) {
      await writePkg(projectPath, pkgJson as any) // tslint:disable-line
    },
  }
}
