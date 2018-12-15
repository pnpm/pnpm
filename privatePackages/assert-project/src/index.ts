import assertStore from '@pnpm/assert-store'
import { read as readModules } from '@pnpm/modules-yaml'
import path = require('path')
import exists = require('path-exists')
import readYamlFile from 'read-yaml-file'
import { Test } from 'tape'
import writePkg = require('write-pkg')
import isExecutable from './isExecutable'

export { isExecutable }

export default (t: Test, projectPath: string, encodedRegistryName?: string) => {
  const ern = encodedRegistryName || 'localhost+4873'
  const modules = path.join(projectPath, 'node_modules')

  let cachedStore: {
    getStorePath (): Promise<string>;
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
      cachedStore = assertStore(t, storePath, ern)
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
      return store.getStorePath()
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
      const store = await getStoreInstance()
      return store.storeHasNot(pkgName, version)
    },
    isExecutable (pathToExe: string) {
      return isExecutable(t, path.join(modules, pathToExe))
    },
    async loadCurrentShrinkwrap () {
      try {
        return await readYamlFile<any>(path.join(modules, '.shrinkwrap.yaml')) // tslint:disable-line
      } catch (err) {
        if (err.code === 'ENOENT') return null
        throw err
      }
    },
    loadModules: () => readModules(modules),
    async loadShrinkwrap () {
      try {
        return await readYamlFile<any>(path.join(projectPath, 'shrinkwrap.yaml')) // tslint:disable-line
      } catch (err) {
        if (err.code === 'ENOENT') return null
        throw err
      }
    },
    async writePackageJson (pkgJson: object) {
      await writePkg(projectPath, pkgJson)
    },
  }
}
