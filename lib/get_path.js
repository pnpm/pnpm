var debug = require('debug')('unpm:get_path')
var dirname = require('path').dirname
var readPkgUp = require('read-pkg-up')

/*
 * Returns the path of package.json.
 */

module.exports = function getPath () {
  return readPkgUp()
    .then(function (pkg) {
      debug('root: ' + dirname(pkg.path))
      return dirname(pkg.path)
    })
}

