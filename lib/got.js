var debug = require('debug')('unpm:http')

/*
 * Interface for 'got' with debug logging
 */

module.exports = function got (url, options) {
  debug(url)
  return require('got').apply(this, arguments)
}
