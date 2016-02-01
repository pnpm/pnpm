var symlink = require('./force_symlink')
var dirname = require('path').dirname
var relative = require('path').relative

/*
 * Relative symlink
 */

module.exports = function relSymlink (src, dest) {
  // Turn it into a relative path when not in win32.
  var isWindows = process.platform === 'win32'
  var rel = isWindows ? src : relative(dirname(dest), src)

  return symlink(rel, dest, 'junction')
}
