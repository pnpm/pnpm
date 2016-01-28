var mkdirp = require('mkdirp')
var rimraf = require('rimraf')
var fs = require('fs')
var join = require('path').join
var root = process.cwd()
process.env.ROOT = root

module.exports = function prepare () {
  rimraf.sync(join(root, '.tmp'))
  mkdirp.sync(join(root, '.tmp'))
  fs.writeFileSync(join(root, '.tmp', 'package.json'), '{}', 'utf-8')
  process.chdir(join(root, '.tmp'))
}
