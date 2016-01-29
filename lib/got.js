var debug = require('debug')('pnpm:http')
var assign = require('object-assign')

var cache = {}

/*
 * Interface for 'got' with debug logging
 */

function got (url, options) {
  debug(url)
  var key = JSON.stringify([ url, options ])
  if (!cache[key]) cache[key] = require('got')(url, extend(options))
  return cache[key]
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
      'user-agent': 'https://github.com/rstacruz/pnpm.js'
    })
  })
}

module.exports = got
