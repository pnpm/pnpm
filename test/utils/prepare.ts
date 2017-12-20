import mkdirp = require('mkdirp')
import fs = require('fs')
import path = require('path')
import {stripIndent} from 'common-tags'
import {Test} from 'tape'
import exists = require('path-exists')
import loadYamlFile = require('load-yaml-file')
import isExecutable from './isExecutable'
import writePkg = require('write-pkg')

// the testing folder should be outside of the project to avoid lookup in the project's node_modules
const tmpPath = path.join(__dirname, '..', '..', '..', '.tmp')
mkdirp.sync(tmpPath)

let dirNumber = 0

export function tempDir (t: Test) {
  dirNumber++
  const dirname = dirNumber.toString()
  const tmpDir = path.join(tmpPath, dirname)
  mkdirp.sync(tmpDir)

  t.pass(`create testing dir ${dirname}`)

  process.chdir(tmpDir)

  return tmpDir
}

export default function prepare (t: Test, pkg?: Object | Object[], pkgTmpPath?: string) {
  pkgTmpPath = pkgTmpPath || path.join(tempDir(t), 'project')

  if (Array.isArray(pkg)) {
    const dirname = path.dirname(pkgTmpPath)
    const result = {}
    for (let aPkg of pkg) {
      result[aPkg['name']] = prepare(t, aPkg, path.join(dirname, aPkg['name']))
    }
    process.chdir('..')
    return result
  }
  mkdirp.sync(pkgTmpPath)
  writePkg.sync(pkgTmpPath, Object.assign({name: 'project', version: '0.0.0'}, pkg))
  process.chdir(pkgTmpPath)

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
      t.notOk(await exists(path.join(modules, pkgName)), `${pkgName} is not in node_modules`)
    },
    getStorePath: async function () {
      if (!cachedStorePath) {
        const modulesYaml = await loadYamlFile(path.join(modules, '.modules.yaml'))
        if (!modulesYaml) {
          throw new Error('Cannot find module store')
        }
        cachedStorePath = modulesYaml['store']
      }
      return cachedStorePath
    },
    resolve: async function (pkgName: string, version?: string, relativePath?: string) {
      const pkgFolder = version ? path.join('localhost+4873', pkgName, version) : pkgName
      if (relativePath) {
        return path.join(await project.getStorePath(), pkgFolder, 'package', relativePath)
      }
      return path.join(await project.getStorePath(), pkgFolder, 'package')
    },
    storeHas: async function (pkgName: string, version?: string) {
      t.ok(await exists(await project.resolve(pkgName, version)), `${pkgName}@${version} is in store`)
    },
    storeHasNot: async function (pkgName: string, version?: string) {
      try {
        t.notOk(await exists(await project.resolve(pkgName, version)), `${pkgName}@${version} is not in store`)
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
