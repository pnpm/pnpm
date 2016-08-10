var rimraf = require('thenify')(require('rimraf'))
var fs = require('mz/fs')

/*
 * Removes `path`.
 * If it's a symlink, remove its destination as well.
 */

module.exports = function obliterate (path) {
  return fs.lstat(path)
    .then(stat => {
      if (stat.isSymbolicLink()) {
        return fs.readlink(path)
          .then(realpath => rimraf(realpath))
      } else {
        return rimraf(path)
      }
    })
    .catch(err => {
      if (err.code !== 'ENOENT') throw err
    })
}
