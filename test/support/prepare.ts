import mkdirp = require('mkdirp')
import fs = require('fs')
import path = require('path')
import {stripIndent} from 'common-tags'

const root = process.cwd()
process.env.ROOT = root

const tmpPath = path.join(root, '.tmp')
mkdirp.sync(tmpPath)
const npmrc = stripIndent` 
  store-path = ./node_modules/.store
  fetch-retries = 5
  fetch-retry-maxtimeout = 180000
  registry =  https://registry.npmjs.org/
  quiet = true
`
fs.writeFileSync(path.join(tmpPath, '.npmrc'), npmrc, 'utf-8')

export default function prepare (pkg?: Object) {
  const pkgTmpPath = path.join(tmpPath, Number(new Date()).toString())
  mkdirp.sync(pkgTmpPath)
  const json = JSON.stringify(pkg || {})
  fs.writeFileSync(path.join(pkgTmpPath, 'package.json'), json, 'utf-8')
  process.chdir(pkgTmpPath)
}
