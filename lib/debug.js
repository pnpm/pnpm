'use strict'
const debug = require('debug')
const logger = require('@zkochan/logger')
const slice = Array.prototype.slice

const debugMap = {}

module.exports = type => {
  debugMap[type] = debug(type)
  return logger.debug.bind(null, type)
}

logger.on('debug', function (ctx, level, type) {
  if (debugMap[type]) {
    const args = slice.call(arguments)
    debugMap[type].apply(debugMap[type], args.slice(3))
  }
})
