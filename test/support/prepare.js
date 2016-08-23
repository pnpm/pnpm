'use strict'
const mkdirp = require('mkdirp')
const fs = require('fs')
const join = require('path').join
const root = process.cwd()
process.env.ROOT = root

module.exports = function prepare (pkg) {
  const tmpPath = join(root, '.tmp', Math.random().toString())
  mkdirp.sync(tmpPath)
  const json = JSON.stringify(pkg || {})
  fs.writeFileSync(join(tmpPath, 'package.json'), json, 'utf-8')
  process.chdir(tmpPath)
}
