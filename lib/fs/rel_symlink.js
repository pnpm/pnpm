'use strict'
const symlink = require('./force_symlink')
const dirname = require('path').dirname
const relative = require('path').relative
const os = require('os')

// Always use "junctions" on Windows. Even though support for "symbolic links" was added in Vista+, users by default
// lack permission to create them
const symlinkType = os.platform() === 'win32' ? 'junction' : 'dir'

/*
 * Relative symlink
 */

module.exports = function relSymlink (src, dest) {
  // Junction points can't be relative
  const rel = symlinkType !== 'junction' ? relative(dirname(dest), src) : src

  return symlink(rel, dest, symlinkType)
}
