var mkdirp = require('mkdirp')
var fs = require('fs')
var join = require('path').join
var root = process.cwd()
process.env.ROOT = root

module.exports = function prepare () {
  var tmpPath = join(root, '.tmp', Math.random().toString())
  mkdirp.sync(tmpPath)
  fs.writeFileSync(join(tmpPath, 'package.json'), '{}', 'utf-8')
  process.chdir(tmpPath)
}
