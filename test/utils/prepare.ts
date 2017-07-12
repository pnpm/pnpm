import mkdirp = require('mkdirp')
import fs = require('fs')
import path = require('path')
import {Test} from 'tape'
import writePkg = require('write-pkg')

// the testing folder should be outside of the project to avoid lookup in the project's node_modules
const tmpPath = path.join(__dirname, '..', '..', '..', '.tmp')
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
  writePkg.sync(pkgTmpPath, Object.assign({name: 'project', version: '0.0.0'}, pkg))
  process.chdir(pkgTmpPath)
  t.pass(`create testing package ${dirname}`)
}
