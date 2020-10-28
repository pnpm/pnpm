import assertStore from '@pnpm/assert-store'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { Lockfile, ProjectSnapshot } from '@pnpm/lockfile-types'
import { Modules, read as readModules } from '@pnpm/modules-yaml'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import readYamlFile from 'read-yaml-file'
import { Test } from 'tape'
import isExecutable from './isExecutable'
import path = require('path')
import exists = require('path-exists')
import writePkg = require('write-pkg')

export { isExecutable, Modules }

export type RawLockfile = Lockfile & Partial<ProjectSnapshot>

export interface Project {
  requireModule: NodeRequireFunction
  has: (pkgName: string, modulesDir?: string) => Promise<void>
  hasNot: (pkgName: string, modulesDir?: string) => Promise<void>
  getStorePath: () => Promise<string>
  resolve: (pkgName: string, version?: string, relativePath?: string) => Promise<string>
  getPkgIndexFilePath: (pkgName: string, version?: string) => Promise<string>
  cafsHas: (pkgName: string, version?: string) => Promise<void>
  cafsHasNot: (pkgName: string, version?: string) => Promise<void>
  storeHas: (pkgName: string, version?: string) => Promise<string>
  storeHasNot: (pkgName: string, version?: string) => Promise<void>
  isExecutable: (pathToExe: string) => Promise<void>
  /**
   * TODO: Remove the `Required<T>` cast.
   *
   * https://github.com/microsoft/TypeScript/pull/32695 might help with this.
   */
  readCurrentLockfile: () => Promise<Required<RawLockfile>>
  readModulesManifest: () => Promise<Modules | null>
  /**
   * TODO: Remove the `Required<T>` cast.
   *
   * https://github.com/microsoft/TypeScript/pull/32695 might help with this.
   */
  readLockfile: () => Promise<Required<RawLockfile>>
  writePackageJson: (pkgJson: object) => Promise<void>
}

export default (t: Test | undefined, projectPath: string, encodedRegistryName?: string): Project => {
  const ern = encodedRegistryName ?? `localhost+${REGISTRY_MOCK_PORT}`
  const modules = path.join(projectPath, 'node_modules')

  let cachedStore: {
    storePath: string
    getPkgIndexFilePath: (pkgName: string, version?: string) => Promise<string>
    cafsHas: (pkgName: string, version?: string | undefined) => Promise<void>
    cafsHasNot: (pkgName: string, version?: string | undefined) => Promise<void>
    storeHas: (pkgName: string, version?: string | undefined) => Promise<void>
    storeHasNot: (pkgName: string, version?: string | undefined) => Promise<void>
    resolve: (pkgName: string, version?: string | undefined, relativePath?: string | undefined) => Promise<string>
  }
  async function getStoreInstance () {
    if (!cachedStore) {
      const modulesYaml = await readModules(modules)
      if (!modulesYaml) {
        throw new Error(`Cannot find module store. No .modules.yaml found at "${modules}"`)
      }
      const storePath = modulesYaml.storeDir
      cachedStore = {
        storePath,
        ...assertStore(t, storePath, ern),
      }
    }
    return cachedStore
  }
  async function getVirtualStoreDir () {
    const modulesYaml = await readModules(modules)
    if (!modulesYaml) {
      return path.join(modules, '.pnpm')
    }
    return modulesYaml.virtualStoreDir
  }

  // eslint-disable-next-line
  const ok = t ? t.ok : (value: any) => expect(value).toBeTruthy()
  // eslint-disable-next-line
  const notOk = t ? t.notOk : (value: any) => expect(value).toBeFalsy()
  return {
    requireModule (pkgName: string) {
      return require(path.join(modules, pkgName))
    },
    async has (pkgName: string, _modulesDir?: string) {
      const md = _modulesDir ? path.join(projectPath, _modulesDir) : modules
      ok(await exists(path.join(md, pkgName)), `${pkgName} is in ${md}`)
    },
    async hasNot (pkgName: string, _modulesDir?: string) {
      const md = _modulesDir ? path.join(projectPath, _modulesDir) : modules
      notOk(await exists(path.join(md, pkgName)), `${pkgName} is not in ${md}`)
    },
    async getStorePath () {
      const store = await getStoreInstance()
      return store.storePath
    },
    async resolve (pkgName: string, version?: string, relativePath?: string) {
      const store = await getStoreInstance()
      return store.resolve(pkgName, version, relativePath)
    },
    async getPkgIndexFilePath (pkgName: string, version?: string): Promise<string> {
      const store = await getStoreInstance()
      return store.getPkgIndexFilePath(pkgName, version)
    },
    async cafsHas (pkgName: string, version?: string) {
      const store = await getStoreInstance()
      return store.cafsHas(pkgName, version)
    },
    async cafsHasNot (pkgName: string, version?: string) {
      const store = await getStoreInstance()
      return store.cafsHasNot(pkgName, version)
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
          t?.pass(`${pkgName}@${version ?? ''} is not in store (store does not even exist)`)
          return
        }
        throw err
      }
    },
    isExecutable (pathToExe: string) {
      return isExecutable(ok, path.join(modules, pathToExe))
    },
    async readCurrentLockfile () {
      try {
        return await readYamlFile(path.join(await getVirtualStoreDir(), 'lock.yaml')) // eslint-disable-line
      } catch (err) {
        if (err.code === 'ENOENT') return null!
        throw err
      }
    },
    readModulesManifest: () => readModules(modules),
    async readLockfile () {
      try {
        return await readYamlFile(path.join(projectPath, WANTED_LOCKFILE)) // eslint-disable-line
      } catch (err) {
        if (err.code === 'ENOENT') return null!
        throw err
      }
    },
    writePackageJson (pkgJson: object) {
      return writePkg(projectPath, pkgJson as any) // eslint-disable-line
    },
  }
}
