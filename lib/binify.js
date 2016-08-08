'use strict'
module.exports = binify

/*
 * Returns bins for a package in a standard object format. This normalizes
 * between npm's string and object formats.
 *
 *    binify({ name: 'rimraf', bin: 'cmd.js' })
 *    => { rimraf: 'cmd.js' }
 *
 *    binify({ name: 'rmrf', bin: { rmrf: 'cmd.js' } })
 *    => { rmrf: 'cmd.js' }
 */

function binify (pkg) {
  if (typeof pkg.bin === 'string') {
    var obj = {}
    obj[pkg.name] = pkg.bin
    return obj
  }

  return pkg.bin || {}
}
