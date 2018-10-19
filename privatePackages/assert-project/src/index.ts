import { read as readModules } from '@pnpm/modules-yaml'
import loadYamlFile = require('load-yaml-file')
import path = require('path')
import exists = require('path-exists')
import { Test } from 'tape'
import writePkg = require('write-pkg')
import isExecutable from './isExecutable'

export { isExecutable }

export default (t: Test, projectPath: string, encodedRegistryName?: string) => {
  const ern = encodedRegistryName || 'localhost+4873'
  const modules = path.join(projectPath, 'node_modules')
  let cachedStorePath: string
  const project = {
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
      if (!cachedStorePath) {
        const modulesYaml = await readModules(modules)
        if (!modulesYaml) {
          throw new Error(`Cannot find module store. No .modules.yaml found at "${modules}"`)
        }
        cachedStorePath = modulesYaml.store
      }
      return cachedStorePath
    },
    async resolve (pkgName: string, version?: string, relativePath?: string) {
      const pkgFolder = version ? path.join(ern, pkgName, version) : pkgName
      if (relativePath) {
        return path.join(await project.getStorePath(), pkgFolder, 'package', relativePath)
      }
      return path.join(await project.getStorePath(), pkgFolder, 'package')
    },
    async storeHas (pkgName: string, version?: string) {
      const pathToCheck = await project.resolve(pkgName, version)
      t.ok(await exists(pathToCheck), `${pkgName}@${version} is in store (at ${pathToCheck})`)
    },
    async storeHasNot (pkgName: string, version?: string) {
      try {
        const pathToCheck = await project.resolve(pkgName, version)
        t.notOk(await exists(pathToCheck), `${pkgName}@${version} is not in store (at ${pathToCheck})`)
      } catch (err) {
        if (err.message.startsWith('Cannot find module store')) {
          t.pass(`${pkgName}@${version} is not in store`)
          return
        }
        throw err
      }
    },
    isExecutable (pathToExe: string) {
      return isExecutable(t, path.join(modules, pathToExe))
    },
    async loadCurrentShrinkwrap () {
      try {
        return await loadYamlFile<any>(path.join(modules, '.shrinkwrap.yaml')) // tslint:disable-line
      } catch (err) {
        if (err.code === 'ENOENT') return null
        throw err
      }
    },
    loadModules: () => readModules(modules),
    async loadShrinkwrap () {
      try {
        return await loadYamlFile<any>(path.join(projectPath, 'shrinkwrap.yaml')) // tslint:disable-line
      } catch (err) {
        if (err.code === 'ENOENT') return null
        throw err
      }
    },
    async writePackageJson (pkgJson: object) {
      await writePkg(projectPath, pkgJson)
    },
  }
  return project
}
