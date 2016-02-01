var Promise = require('./promise')
var debug = require('debug')('pnpm:http')
var assign = require('object-assign')
var throat = require('throat')
var got = require('got')
var config = require('./config')

var cache = {}

var throater = throat(+config.concurrency)

/*
 * waits in line
 */

exports.get = function (url, options) {
  var key = JSON.stringify([ url, options ])
  if (!cache[key]) {
    cache[key] = new Promise(resolve => {
      throater(_ => {
        debug(url)
        var promise = got(url, extend(options))
        resolve({ promise: promise })
        return promise
      })
    })
  }
  return cache[key]
}

/*
 * like require('got').stream, but throated
 */

exports.getStream = function (url, options) {
  return new Promise(resolve => {
    throater(_ => {
      debug(url, '[stream]')
      var stream = got.stream(url, extend(options))
      resolve(stream)
      return waiter(stream)
    })
  })
}

function waiter (stream) {
  return new Promise((resolve, reject) => {
    stream
      .on('end', resolve)
      .on('error', reject)
  })
}

/*
 * Extends `got` options with User Agent headers and stuff
 */

function extend (options) {
  if (!options) options = {}
  return assign({}, options, {
    headers: assign({}, options.headers || {}, {
      'user-agent': 'https://github.com/rstacruz/pnpm'
    })
  })
}
