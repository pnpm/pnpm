'use strict'

var fs = require('fs')
var thenify = require('thenify')
var debug = require('debug')('pnpm:symlink')

/*
 * Creates a symlink. Re-link if a symlink already exists at the supplied
 * srcPath. API compatible with [`fs#symlink`](https://nodejs.org/api/fs.html#fs_fs_symlink_srcpath_dstpath_type_callback).
 */

function forceSymlink (srcPath, dstPath, type, cb) {
  debug('%s -> %s', srcPath, dstPath)
  try {
    fs.symlinkSync(srcPath, dstPath, type)
    cb()
  } catch (err) {
    if (err.code !== 'EEXIST') return cb(err)

    fs.readlink(dstPath, function (err, linkString) {
      if (err || srcPath === linkString) return cb(err)

      fs.unlink(dstPath, function (err) {
        if (err) return cb(err)
        forceSymlink(srcPath, dstPath, type, cb)
      })
    })
  }
}

module.exports = thenify(forceSymlink)
