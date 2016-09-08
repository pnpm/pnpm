import fs = require('mz/fs')
import path = require('path')
import linkBins from './link_bins'

export default function linkBundledDeps (root) {
  const nodeModules = path.join(root, 'node_modules')

  return isDir(nodeModules, _ =>
    Promise.all(fs.readdirSync(nodeModules).map(mod =>
      isDir(path.join(nodeModules, mod), _ =>
        symlinkBundledDep(nodeModules, path.join(nodeModules, mod))
      )
    )))
}

function symlinkBundledDep (nodeModules, submod) {
  return linkBins(nodeModules)
}

function isDir (path, fn) {
  return fs.stat(path)
  .then(stat => {
    if (!stat.isDirectory()) return Promise.resolve()
    return fn()
  })
  .catch(err => { if (err.code !== 'ENOENT') throw err })
}
