import mkdirp = require('mkdirp')
import fs = require('fs')
import path = require('path')
import {stripIndent} from 'common-tags'
import {Test} from 'tape'
import exists = require('path-exists')
import loadYamlFile = require('load-yaml-file')
import {Modules, read as readModules} from '../../src/fs/modulesController'
import isExecutable from './isExecutable'

// the testing folder should be outside of the project to avoid lookup in the project's node_modules
const tmpPath = path.resolve('..', '.tmp')
mkdirp.sync(tmpPath)

let dirNumber = 0

export default function prepare (t: Test, pkg?: Object) {
  process.env.NPM_CONFIG_REGISTRY = 'http://localhost:4873/'
  process.env.NPM_CONFIG_STORE_PATH = '../.store'
  process.env.NPM_CONFIG_SILENT = 'true'

  dirNumber++
  const dirname = dirNumber.toString()
  const pkgTmpPath = path.join(tmpPath, dirname, 'project')
  mkdirp.sync(pkgTmpPath)
  const json = JSON.stringify(Object.assign({name: 'foo', version: '0.0.0'}, pkg))
  fs.writeFileSync(path.join(pkgTmpPath, 'package.json'), json, 'utf-8')
  process.chdir(pkgTmpPath)
  t.pass(`create testing package ${dirname}`)

  const modules = path.join(pkgTmpPath, 'node_modules')
  let cachedStorePath: string
  const project = {
    requireModule (pkgName: string) {
      return require(path.join(modules, pkgName))
    },
    has: async function (pkgName: string) {
      t.ok(await exists(path.join(modules, pkgName)), `${pkgName} is in node_modules`)
    },
    hasNot: async function (pkgName: string) {
      t.ok(!await exists(path.join(modules, pkgName)), `${pkgName} is not in node_modules`)
    },
    getStorePath: async function () {
      if (!cachedStorePath) {
        const modulesYaml = await readModules(modules)
        if (!modulesYaml) {
          throw new Error('Cannot find module store')
        }
        cachedStorePath = modulesYaml.storePath
      }
      return cachedStorePath
    },
    resolve: async function (pkgName: string, version?: string, relativePath?: string) {
      const pkgFolder = version ? path.join('localhost+4873', pkgName, version) : pkgName
      if (relativePath) {
        return path.join(await project.getStorePath(), pkgFolder, relativePath)
      }
      return path.join(await project.getStorePath(), pkgFolder)
    },
    storeHas: async function (pkgName: string, version?: string) {
      t.ok(await exists(await project.resolve(pkgName, version)), `${pkgName}@${version} is in store`)
    },
    storeHasNot: async function (pkgName: string, version?: string) {
      try {
        t.ok(!await exists(await project.resolve(pkgName, version)), `${pkgName}@${version} is not in store`)
      } catch (err) {
        if (err.message === 'Cannot find module store') {
          t.pass(`${pkgName}@${version} is not in store`)
          return
        }
        throw err
      }
    },
    isExecutable: function (pathToExe: string) {
      return isExecutable(t, path.join(modules, pathToExe))
    },
    loadShrinkwrap: async () => {
      try {
        return await loadYamlFile<any>('shrinkwrap.yaml') // tslint:disable-line
      } catch (err) {
        if (err.code === 'ENOENT') return null
        throw err
      }
    },
  }
  return project
}
