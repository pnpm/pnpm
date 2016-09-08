import fs = require('mz/fs')

/*
 * Removes a symlink
 */

export default function unsymlink (path) {
  return fs.lstat(path)
  .then(stat => {
    if (stat.isSymbolicLink()) return fs.unlink(path)
    throw new Error('Can\'t unlink ' + path)
  })
  .catch(err => {
    if (err.code !== 'ENOENT') throw err
  })
}
