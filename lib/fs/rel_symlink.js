var symlink = require('./force_symlink')
var dirname = require('path').dirname
var relative = require('path').relative
var os = require('os')

// Always use "junctions" on Windows. Even though support for "symbolic links" was added in Vista+, users by default
// lack permission to create them
var symlinkType = os.platform() === 'win32' ? 'junction' : 'dir'

/*
 * Relative symlink
 */

module.exports = function relSymlink (src, dest) {
  // Junction points can't be relative
  var rel = symlinkType !== 'junction' ? relative(dirname(dest), src) : src

  return symlink(rel, dest, symlinkType)
}
