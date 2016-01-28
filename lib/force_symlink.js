'use strict'

var fs = require('fs')
var thenify = require('thenify')
var debug = require('debug')('pnpm:install')

/*
 * Creates a symlink. Re-link if a symlink already exists at the supplied
 * srcPath. API compatible with [`fs#symlink`](https://nodejs.org/api/fs.html#fs_fs_symlink_srcpath_dstpath_type_callback).
 */

function forceSymlink (srcPath, dstPath, type, cb) {
  debug('symlink %s -> %s', srcPath, dstPath)
  type = typeof type === 'string' ? type : null
  cb = arguments[arguments.length - 1] || function () {}
  fs.symlink(srcPath, dstPath, type, function (err) {
    if (!err || err.code !== 'EEXIST') return cb(err)

    fs.readlink(dstPath, function (err, linkString) {
      if (err || srcPath === linkString) return cb(err)

      fs.unlink(dstPath, function (err) {
        if (err) return cb(err)
        forceSymlink(srcPath, dstPath, cb)
      })
    })
  })
}

module.exports = thenify(forceSymlink)
