var mkdirp = require('mkdirp')
var fs = require('fs')
var join = require('path').join
var root = process.cwd()
process.env.ROOT = root

module.exports = function prepare (pkg) {
  var tmpPath = join(root, '.tmp', Math.random().toString())
  mkdirp.sync(tmpPath)
  var json = JSON.stringify(pkg || {})
  fs.writeFileSync(join(tmpPath, 'package.json'), json, 'utf-8')
  process.chdir(tmpPath)
}
