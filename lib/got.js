var debug = require('debug')('unpm:http')
var throat = require('throat')
var gotEx = throat(4, require('got'))
var gotStreamEx = throat(4, require('got').stream)
var assign = require('object-assign')

/*
 * Interface for 'got' with debug logging
 */

function got (url, options) {
  debug(url)
  return gotEx(url, extend(options))
}

/*
 * like require('got').stream, but throated
 */

got.stream = function (url, options) {
  debug(url)
  return require('got').stream(url, extend(options))
}

/*
 * Extends `got` options with User Agent headers and stuff
 */

function extend (options) {
  if (!options) options = {}
  return assign({}, options, {
    headers: assign({}, options.headers || {}, {
      'user-agent': 'https://github.com/rstacruz'
    })
  })
}

module.exports = got
