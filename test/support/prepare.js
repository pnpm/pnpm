'use strict'
const mkdirp = require('mkdirp')
const fs = require('fs')
const join = require('path').join
const root = process.cwd()
process.env.ROOT = root

const tmpPath = join(root, '.tmp')
mkdirp.sync(tmpPath)
const npmrc = [
  'store-path=./node_modules/.store',
  'fetch-retries=5',
  'fetch-retry-maxtimeout=180000'
]
fs.writeFileSync(join(tmpPath, '.npmrc'), npmrc, 'utf-8')

module.exports = function prepare (pkg) {
  const pkgTmpPath = join(tmpPath, Number(new Date()).toString())
  mkdirp.sync(pkgTmpPath)
  const json = JSON.stringify(pkg || {})
  fs.writeFileSync(join(pkgTmpPath, 'package.json'), json, 'utf-8')
  process.chdir(pkgTmpPath)
}
