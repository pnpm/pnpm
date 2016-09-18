import fs = require('mz/fs')
import {Stats} from 'fs'

/**
 * Removes a symlink
 */
export default function unsymlink (path: string) {
  return fs.lstat(path)
  .then((stat: Stats) => {
    if (stat.isSymbolicLink()) return fs.unlink(path)
    throw new Error('Can\'t unlink ' + path)
  })
  .catch((err: NodeJS.ErrnoException) => {
    if (err.code !== 'ENOENT') throw err
  })
}
