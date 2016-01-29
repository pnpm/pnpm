var symlink = require('./force_symlink')
var dirname = require('path').dirname
var relative = require('path').relative

/*
 * Relative symlink
 */

module.exports = function relSymlink (src, dest) {
  var rel = relative(dirname(dest), src)
  return symlink(rel, dest)
}
