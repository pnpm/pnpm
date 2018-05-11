import assertProject from '@pnpm/assert-project'
import mkdirp = require('mkdirp')
import path = require('path')
import {Test} from 'tape'
import writePkg = require('write-pkg')

// the testing folder should be outside of the project to avoid lookup in the project's node_modules
const tmpPath = path.join(__dirname, '..', '..', '..', '.tmp')
mkdirp.sync(tmpPath)

let dirNumber = 0

export default function prepare (t: Test, pkg?: object) {
  process.env.NPM_CONFIG_REGISTRY = 'http://localhost:4873/'
  process.env.NPM_CONFIG_STORE_PATH = '../.store'
  process.env.NPM_CONFIG_SILENT = 'true'

  dirNumber++
  const dirname = dirNumber.toString()
  const pkgTmpPath = path.join(tmpPath, dirname, 'project')
  mkdirp.sync(pkgTmpPath)
  let pkgJson = {name: 'project', version: '0.0.0', ...pkg}
  writePkg.sync(pkgTmpPath, pkgJson)
  process.chdir(pkgTmpPath)
  t.pass(`create testing package ${dirname}`)

  return {
    ...assertProject(t, pkgTmpPath),
    async rewriteDependencies (deps: object) {
      pkgJson = Object.assign(pkgJson, { dependencies: deps })
      writePkg.sync(pkgTmpPath, pkgJson)
    },
  }
}
