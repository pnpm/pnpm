var debug = require('debug')('unpm:mkdirp')

/*
 * mkdir -p as a promise.
 */

module.exports = function mkdirp (path) {
  return new Promise(function (resolve, reject) {
    debug(path)
    require('mkdirp')(path, function (err) {
      if (err) reject(err)
      else resolve(path)
    })
  })
}
