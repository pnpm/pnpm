import fs = require('fs')
import thenify = require('thenify')
import createDebug from '../debug'
const debug = createDebug('pnpm:symlink')

/*
 * Creates a symlink. Re-link if a symlink already exists at the supplied
 * srcPath. API compatible with [`fs#symlink`](https://nodejs.org/api/fs.html#fs_fs_symlink_srcpath_dstpath_type_callback).
 */

function forceSymlink (srcPath, dstPath, type, cb) {
  debug(`${srcPath} -> ${dstPath}`)
  try {
    fs.symlinkSync(srcPath, dstPath, type)
    cb()
  } catch (err) {
    if (err.code !== 'EEXIST') return cb(err)

    fs.readlink(dstPath, (err, linkString) => {
      if (err || srcPath === linkString) return cb(err)

      fs.unlink(dstPath, err => {
        if (err) return cb(err)
        forceSymlink(srcPath, dstPath, type, cb)
      })
    })
  }
}

export default thenify(forceSymlink)
