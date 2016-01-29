var Promise = require('../promise')
var fs = require('mz/fs')
var join = require('path').join
var linkBins = require('./link_bins')

module.exports = function linkBundledDeps (root) {
  var nodeModules = join(root, 'node_modules')

  return isDir(nodeModules, _ =>
    Promise.all(fs.readdir(nodeModules).map(mod =>
      isDir(join(nodeModules, mod), _ =>
        symlinkBundledDep(nodeModules, join(nodeModules, mod))
      )
    )))
}

function symlinkBundledDep (nodeModules, submod) {
  return linkBins(nodeModules, submod, submod)
}

function isDir (path, fn) {
  return fs.stat(path)
  .then(stat => {
    if (!stat.isDirectory()) return Promise.resolve()
    return fn()
  })
  .catch(err => { if (err.code !== 'ENOENT') throw err })
}

